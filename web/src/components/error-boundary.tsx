import * as React from "react";

interface ErrorBoundaryProps {
	children: React.ReactNode;
}

interface ErrorBoundaryState {
	hasError: boolean;
}

export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
	constructor(props: ErrorBoundaryProps) {
		super(props);
		this.state = { hasError: false };
	}

	static getDerivedStateFromError(): ErrorBoundaryState {
		return { hasError: true };
	}

	componentDidCatch(error: Error, info: React.ErrorInfo) {
		console.error("Uncaught error:", error, info);
	}

	render() {
		if (this.state.hasError) {
			return (
				<div className="flex min-h-screen flex-col items-center justify-center p-6 text-center">
					<h1 className="mb-4 text-2xl font-bold">出错了</h1>
					<p className="mb-6 text-muted-foreground">应用发生错误，请刷新页面重试。</p>
					<button
						type="button"
						onClick={() => window.location.reload()}
						className="rounded-md bg-primary px-4 py-2 text-primary-foreground hover:bg-primary/90"
					>
						刷新页面
					</button>
				</div>
			);
		}

		return this.props.children;
	}
}
