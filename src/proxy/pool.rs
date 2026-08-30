//! 上游连接池：按 `scheme://host:port` 隔离的 HTTP/1.1 空闲连接复用。
//!
//! 依赖 hyper `http1::handshake` 返回的 `SendRequest`（`Clone` 共享底层连接）：
//! - 连接空闲时多个 clone 排队复用，忙时 hyper 内部排队；
//! - sender 全部 drop 后，hyper 的 conn 驱动任务自动关闭底层连接（即释放）；
//! - `is_closed()` 返回 false 表示连接健康可继续复用，返回 true 表示已终止（丢弃）。
//!
//! 归还时机绑定在响应体生命周期：`PooledBody::poll_frame` 读到 EOF 或流错误时，
//! 若连接仍健康则归还池，否则丢弃；客户端提前断开（body 未读完被 drop）时不归还，
//! 避免复用带残留数据的连接。

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1;

/// 池化后的上游响应体：委托 `Incoming`，在读到 EOF 或流错误时把连接归还池。
pub struct PooledBody {
    inner: Incoming,
    key: String,
    sender: Option<http1::SendRequest<Full<Bytes>>>,
    pool: UpstreamPool,
    /// 是否已归还/已丢弃，避免 EOF 后再次触发。
    settled: bool,
}

impl PooledBody {
    pub fn new(
        inner: Incoming,
        key: String,
        sender: http1::SendRequest<Full<Bytes>>,
        pool: UpstreamPool,
    ) -> Self {
        Self {
            inner,
            key,
            sender: Some(sender),
            pool,
            settled: false,
        }
    }

    /// 把连接归还池或丢弃。仅在流自然结束（EOF）或流错误时调用一次；
    /// 客户端提前 drop body 不走这里（`sender` 随之 drop，连接被 hyper 关闭）。
    fn settle(&mut self) {
        if self.settled {
            return;
        }
        self.settled = true;
        if let Some(sender) = self.sender.take() {
            if sender.is_closed() {
                tracing::debug!("upstream connection closed by peer, not pooled");
            } else {
                self.pool.inner.release(&self.key, sender);
            }
        }
    }
}

impl http_body::Body for PooledBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        match &polled {
            Poll::Ready(None) | Poll::Ready(Some(Err(_))) => self.settle(),
            _ => {}
        }
        polled
    }
}

/// 池中的一条空闲连接。
struct PooledConn {
    sender: http1::SendRequest<Full<Bytes>>,
    /// 该连接最近一次归还（回到空闲池）的时刻，用于空闲超时判定。
    last_idle_at: Instant,
}

#[derive(Clone)]
pub struct UpstreamPool {
    inner: Arc<UpstreamPoolInner>,
}

pub struct UpstreamPoolInner {
    idle: Mutex<HashMap<String, Vec<PooledConn>>>,
    idle_timeout: Duration,
}

impl UpstreamPool {
    /// 创建一个空闲超时（默认建议 600s）的连接池，并启动后台空闲清理任务。
    pub fn new(idle_timeout: Duration) -> Self {
        let pool = Self {
            inner: Arc::new(UpstreamPoolInner {
                idle: Mutex::new(HashMap::new()),
                idle_timeout,
            }),
        };
        UpstreamPoolInner::spawn_cleaner(Arc::clone(&pool.inner));
        pool
    }

    /// 取出一条可用连接：弹出第一个未过期且未被对端关闭的连接，
    /// 顺带惰性丢弃该 key 下过期/已死的陈旧连接。
    pub fn checkout(&self, key: &str) -> Option<http1::SendRequest<Full<Bytes>>> {
        self.inner.checkout(key)
    }
}

impl UpstreamPoolInner {
    /// 后台任务：每 60s 扫描一次，释放空闲超过 `idle_timeout` 的连接。
    /// sender drop 后 hyper conn 驱动任务会关闭底层连接，无需显式 close。
    fn spawn_cleaner(inner: Arc<UpstreamPoolInner>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let cutoff = Instant::now() - inner.idle_timeout;
                let mut idle = match inner.idle.lock() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };
                let mut removed = 0usize;
                for list in idle.values_mut() {
                    let before = list.len();
                    list.retain(|conn| conn.last_idle_at > cutoff);
                    removed += before - list.len();
                }
                if removed > 0 {
                    tracing::debug!("released {removed} idle upstream connections");
                }
            }
        });
    }

    /// 取出一条可用连接：弹出第一个未过期且未被对端关闭的连接，
    /// 顺带惰性丢弃该 key 下过期/已死的陈旧连接。
    fn checkout(&self, key: &str) -> Option<http1::SendRequest<Full<Bytes>>> {
        let cutoff = Instant::now() - self.idle_timeout;
        let mut idle = match self.idle.lock() {
            Ok(guard) => guard,
            Err(_) => return None,
        };
        let list = idle.get_mut(key)?;
        loop {
            let conn = match list.pop() {
                Some(conn) => conn,
                None => {
                    idle.remove(key);
                    return None;
                }
            };
            if conn.last_idle_at > cutoff && !conn.sender.is_closed() {
                return Some(conn.sender);
            }
            // 过期或已死：丢弃，继续找下一条。
        }
    }

    /// 归还一条仍健康的连接到空闲池。
    fn release(&self, key: &str, sender: http1::SendRequest<Full<Bytes>>) {
        if let Ok(mut idle) = self.idle.lock() {
            idle.entry(key.to_string()).or_default().push(PooledConn {
                sender,
                last_idle_at: Instant::now(),
            });
        }
    }
}
