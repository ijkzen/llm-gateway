import { useEffect } from "react";
import { useLocation } from "react-router-dom";

/**
 * 路由切换时滚动到页面顶部。
 *
 * SPA 路由切换不会重置浏览器滚动位置：从首页（滚到赛马卡片处）点击行进入
 * 二级页后，window 仍保持旧滚动位置，内容较短时会被 clamp 到新页面底部。
 * location.key 每次导航唯一，用它触发 window.scrollTo(0, 0) 恢复顶部。
 */
export function ScrollToTop() {
	const location = useLocation();

	useEffect(() => {
		window.scrollTo(0, 0);
		// location.key 是本次导航的唯一标识，引用它以声明「导航变化即重滚」。
		void location.key;
	}, [location.key]);

	return null;
}
