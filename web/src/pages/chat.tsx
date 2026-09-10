import { MidEllipsis } from "@/components/mid-ellipsis";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useProviderModels } from "@/hooks/use-provider-models";
import { useProviders } from "@/hooks/use-providers";
import { cn } from "@/lib/utils";
import { ChevronDown, ChevronRight, ChevronUp, Eraser, SendHorizontal, Square } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ChatMessage {
	id: number;
	role: "user" | "assistant";
	content: string;
	reasoning: string;
	/** 无损思考载体（OpenRouter reasoning_details 兼容形状），发送历史时原样回传。 */
	reasoningDetails?: unknown[];
	/** 思考过程折叠区展开态；正文到达或流结束自动折叠（手动切换后不再自动干预）。 */
	reasoningOpen: boolean;
	reasoningTouched: boolean;
	stopped?: boolean;
	error?: string;
}

/** 从 SSE 事件文本中提取 data 载荷（非 data 行与空行忽略）。
 *  21-03：SSE 规范允许多个 data 行组成一个载荷（以 \n 拼接）；网关当前单行输出，
 *  这里按规范合并以防上游/未来变更导致载荷被截断。 */
function eventData(event: string): string | null {
	const parts = event
		.split("\n")
		.filter((l) => l.startsWith("data:"))
		.map((l) => l.slice(5).replace(/^ /, ""));
	if (parts.length === 0) return null;
	return parts.join("\n");
}

/** 解析一个 OpenAI chunk 的 delta 增量。思考字段同时兼容 DeepSeek 风格
 * （reasoning_content，网关透传 DeepSeek/Kimi 等原生键名）与 OpenRouter 风格
 * （reasoning，网关透传 Command Code 等聚合器键名）；reasoning_details 为
 * 网关无损思考载体，随历史原样回传。 */
function deltaOf(data: string): {
	reasoning?: string;
	content?: string;
	details?: unknown[];
	error?: string;
} {
	if (data === "[DONE]") return {};
	const parsed: unknown = JSON.parse(data);
	// 21-02：网关在流中转换失败时发 `{"error":{...}}` 帧——此前无 choices 被
	// 静默忽略，表现为内容戛然而止；这里提取为错误展示。
	const errorField = (parsed as { error?: unknown }).error;
	if (errorField) {
		const message =
			typeof errorField === "string"
				? errorField
				: ((errorField as { message?: string }).message ?? JSON.stringify(errorField));
		return { error: message };
	}
	const delta = (parsed as { choices?: Array<{ delta?: Record<string, unknown> }> }).choices?.[0]
		?.delta;
	const reasoning = delta?.reasoning_content ?? delta?.reasoning;
	const content = delta?.content;
	const details = Array.isArray(delta?.reasoning_details) ? delta.reasoning_details : undefined;
	return {
		reasoning: typeof reasoning === "string" && reasoning ? reasoning : undefined,
		content: typeof content === "string" && content ? content : undefined,
		details,
	};
}

export default function ChatPage() {
	const { t } = useTranslation();
	const { data: providers } = useProviders();
	const { data: models } = useProviderModels();
	const [messages, setMessages] = useState<ChatMessage[]>([]);
	const [input, setInput] = useState("");
	const [modelKey, setModelKey] = useState("");
	const [pickerOpen, setPickerOpen] = useState(false);
	/** 折叠的供应商分组（providerId 集合）。 */
	const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set());
	const [streaming, setStreaming] = useState(false);
	const abortRef = useRef<AbortController | null>(null);
	const nextIdRef = useRef(1);

	// 按启用供应商分组（provider_model 无独立启停，随供应商）；供应商顺序沿用列表序。
	const groups = useMemo(() => {
		return (providers ?? [])
			.filter((p) => p.enable)
			.map((p) => ({
				providerId: p.id,
				providerName: p.name,
				models: (models ?? [])
					.filter((m) => m.providerId === p.id)
					.map((m) => ({
						key: `${p.id}:${m.modelId}`,
						providerName: p.name,
						// 分组内只显示模型 ID，供应商名由分组标题与触发器承担。
						label: m.providerModelId,
					})),
			}))
			.filter((group) => group.models.length > 0);
	}, [providers, models]);
	const selected = groups.flatMap((g) => g.models).find((m) => m.key === modelKey);

	const patchLast = useCallback((patch: (msg: ChatMessage) => ChatMessage) => {
		setMessages((prev) => {
			const last = prev[prev.length - 1];
			if (!last || last.role !== "assistant") return prev;
			return [...prev.slice(0, -1), patch(last)];
		});
	}, []);

	const stop = useCallback(() => {
		abortRef.current?.abort();
	}, []);

	const send = async () => {
		const text = input.trim();
		if (!text || !modelKey || streaming) return;
		const [providerId, modelId] = modelKey.split(":").map(Number);
		// assistant 消息携带的思考载体原样回传（无损续链）；思考文本仅供展示。
		const history = messages.map((m) => ({
			role: m.role,
			content: m.content,
			...(m.role === "assistant" && m.reasoningDetails?.length
				? { reasoning_details: m.reasoningDetails }
				: {}),
		}));
		const controller = new AbortController();
		abortRef.current = controller;
		setInput("");
		setMessages((prev) => [
			...prev,
			{
				id: nextIdRef.current++,
				role: "user",
				content: text,
				reasoning: "",
				reasoningOpen: false,
				reasoningTouched: false,
			},
			{
				id: nextIdRef.current++,
				role: "assistant",
				content: "",
				reasoning: "",
				reasoningOpen: true,
				reasoningTouched: false,
			},
		]);
		setStreaming(true);
		try {
			const res = await fetch("/api/chat/completions", {
				method: "POST",
				headers: { "content-type": "application/json" },
				credentials: "same-origin",
				signal: controller.signal,
				body: JSON.stringify({
					providerId,
					modelId,
					messages: [...history, { role: "user", content: text }],
				}),
			});
			if (!res.ok) {
				const bodyText = await res.text();
				let message = `HTTP ${res.status}`;
				try {
					const parsed = JSON.parse(bodyText) as { msg?: string };
					message = parsed.msg ?? message;
				} catch {
					// 非 JSON 错误体，保留 HTTP 状态兜底
				}
				throw new Error(message);
			}
			const reader = res.body?.getReader();
			if (!reader) throw new Error(t("error.networkError"));
			const decoder = new TextDecoder();
			let buffer = "";
			for (;;) {
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				const events = buffer.split("\n\n");
				buffer = events.pop() ?? "";
				for (const event of events) {
					const data = eventData(event);
					if (!data) continue;
					const delta = deltaOf(data);
					if (delta.error !== undefined) {
						// 21-02：流内错误帧标记到当前消息并结束本轮。
						const message = delta.error;
						patchLast((msg) => ({ ...msg, error: message }));
						break;
					}
					if (delta.reasoning !== undefined) {
						patchLast((msg) => ({
							...msg,
							reasoning: msg.reasoning + delta.reasoning,
							reasoningOpen: msg.reasoningTouched ? msg.reasoningOpen : !msg.content,
						}));
					}
					if (delta.details) {
						const details = delta.details;
						patchLast((msg) => ({
							...msg,
							reasoningDetails: [...(msg.reasoningDetails ?? []), ...details],
						}));
					}
					if (delta.content !== undefined) {
						patchLast((msg) => ({
							...msg,
							content: msg.content + delta.content,
							reasoningOpen: msg.reasoningTouched ? msg.reasoningOpen : false,
						}));
					}
				}
			}
		} catch (err) {
			if (err instanceof DOMException && err.name === "AbortError") {
				patchLast((msg) => ({ ...msg, stopped: true }));
			} else {
				const message = err instanceof Error ? err.message : String(err);
				patchLast((msg) => ({ ...msg, error: message }));
			}
		} finally {
			setStreaming(false);
			abortRef.current = null;
		}
	};

	const toggleReasoning = (id: number) => {
		setMessages((prev) =>
			prev.map((msg) =>
				msg.id === id ? { ...msg, reasoningOpen: !msg.reasoningOpen, reasoningTouched: true } : msg,
			),
		);
	};

	const toggleGroup = (providerId: number) => {
		setCollapsed((prev) => {
			const next = new Set(prev);
			if (next.has(providerId)) {
				next.delete(providerId);
			} else {
				next.add(providerId);
			}
			return next;
		});
	};

	return (
		<div className="flex h-[calc(100vh-10rem)] flex-col gap-4">
			<div className="flex-1 space-y-3 overflow-y-auto rounded-xl border bg-card p-4">
				{messages.length === 0 && (
					<p className="py-16 text-center text-sm text-muted-foreground">{t("chat.emptyHint")}</p>
				)}
				{messages.map((msg) =>
					msg.role === "user" ? (
						<div key={msg.id} className="flex justify-end">
							<div className="max-w-[80%] whitespace-pre-wrap rounded-2xl bg-primary px-4 py-2 text-sm text-primary-foreground">
								{msg.content}
							</div>
						</div>
					) : (
						<div key={msg.id} className="flex justify-start">
							<div className="max-w-[80%] space-y-2 rounded-2xl bg-muted px-4 py-2 text-sm">
								{msg.reasoning && (
									<div>
										<button
											type="button"
											aria-expanded={msg.reasoningOpen}
											onClick={() => toggleReasoning(msg.id)}
											className="text-xs text-muted-foreground transition-colors hover:text-foreground"
										>
											{msg.reasoningOpen
												? t("chat.thinking")
												: `${t("chat.thinking")} · ${msg.reasoning.length}`}
										</button>
										{msg.reasoningOpen && (
											<div className="mt-1 whitespace-pre-wrap border-l-2 pl-2 text-xs text-muted-foreground">
												{msg.reasoning}
											</div>
										)}
									</div>
								)}
								{msg.content && <div className="whitespace-pre-wrap">{msg.content}</div>}
								{msg.stopped && (
									<div className="text-xs text-muted-foreground">{t("chat.stopped")}</div>
								)}
								{msg.error && <div className="text-xs text-destructive">{msg.error}</div>}
							</div>
						</div>
					),
				)}
			</div>
			<div className="flex shrink-0 flex-col gap-2">
				<div className="flex items-center justify-end gap-2">
					<Button
						variant="ghost"
						size="icon"
						className="size-8"
						aria-label={t("chat.clear")}
						title={t("chat.clear")}
						onClick={() => setMessages([])}
						disabled={streaming}
					>
						<Eraser className="size-4" />
					</Button>
					<Popover open={pickerOpen} onOpenChange={setPickerOpen}>
						<PopoverTrigger asChild>
							<Button variant="outline" size="sm" className="max-w-72">
								<MidEllipsis
									text={
										selected
											? `${selected.providerName} / ${selected.label}`
											: t("chat.selectModel")
									}
								/>
								<ChevronUp className="size-4 shrink-0 text-muted-foreground" />
							</Button>
						</PopoverTrigger>
						<PopoverContent side="top" align="end" className="w-72 p-1">
							{groups.length === 0 ? (
								<p className="p-3 text-sm text-muted-foreground">{t("chat.emptyHint")}</p>
							) : (
								<div className="max-h-72 overflow-y-auto">
									{groups.map((group) => {
										const isCollapsed = collapsed.has(group.providerId);
										return (
											<div key={group.providerId}>
												<button
													type="button"
													aria-expanded={!isCollapsed}
													onClick={() => toggleGroup(group.providerId)}
													className="flex w-full items-center gap-1 rounded-md px-2 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
												>
													{isCollapsed ? (
														<ChevronRight className="size-3.5 shrink-0" />
													) : (
														<ChevronDown className="size-3.5 shrink-0" />
													)}
													{group.providerName}
												</button>
												{!isCollapsed &&
													group.models.map((model) => (
														<button
															key={model.key}
															type="button"
															onClick={() => {
																setModelKey(model.key);
																setPickerOpen(false);
															}}
															className={cn(
																"flex w-full items-center rounded-md px-2 py-1.5 pl-6 text-left text-sm transition-colors hover:bg-accent",
																model.key === modelKey && "bg-accent font-medium",
															)}
														>
															<MidEllipsis text={model.label} className="min-w-0" />
														</button>
													))}
											</div>
										);
									})}
								</div>
							)}
						</PopoverContent>
					</Popover>
				</div>
				<div className="relative">
					<textarea
						value={input}
						onChange={(e) => setInput(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
								e.preventDefault();
								send();
							}
						}}
						placeholder={t("chat.inputPlaceholder")}
						rows={2}
						className="w-full resize-none rounded-xl border bg-background px-3 py-2 pr-12 text-sm"
					/>
					{streaming ? (
						<Button
							variant="ghost"
							size="icon"
							className="absolute right-2 bottom-2 size-7"
							aria-label={t("chat.stop")}
							title={t("chat.stop")}
							onClick={stop}
						>
							<Square className="size-3.5 fill-current" />
						</Button>
					) : (
						<Button
							size="icon"
							className="absolute right-2 bottom-2 size-7"
							aria-label={t("chat.send")}
							title={t("chat.send")}
							onClick={send}
							disabled={!input.trim() || !modelKey}
						>
							<SendHorizontal className="size-4" />
						</Button>
					)}
				</div>
			</div>
		</div>
	);
}
