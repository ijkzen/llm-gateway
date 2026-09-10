import { ErrorBoundary } from "@/components/error-boundary";
import { TooltipProvider } from "@/components/ui/tooltip";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import App from "./App";
// 20-09：显式初始化 i18n（此前仅靠 App→use-locale 的副作用链触发，
// 入口重构会静默丢失初始化）。
import "@/i18n";
import "./index.css";
import "./sticky-header.css";

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			staleTime: 1000 * 60 * 5,
			retry: 1,
		},
	},
});

const rootElement = document.getElementById("root");
if (!rootElement) {
	throw new Error("Root element not found");
}

ReactDOM.createRoot(rootElement).render(
	<React.StrictMode>
		<QueryClientProvider client={queryClient}>
			<TooltipProvider delayDuration={200}>
				<BrowserRouter>
					<ErrorBoundary>
						<App />
					</ErrorBoundary>
				</BrowserRouter>
			</TooltipProvider>
		</QueryClientProvider>
	</React.StrictMode>,
);
