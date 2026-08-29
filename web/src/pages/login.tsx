import { useAuthStatus, useInitAdmin, useLogin, useMe } from "@/hooks/use-auth";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { Settings, ShieldCheck } from "lucide-react";
import { useForm } from "react-hook-form";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { z } from "zod";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

const loginSchema = z.object({
	username: z.string().min(1, "请输入用户名"),
	password: z.string().min(1, "请输入密码"),
});

const initSchema = z
	.object({
		username: z.string().min(1, "请输入用户名").max(64, "用户名最多 64 个字符"),
		password: z.string().min(6, "密码至少 6 个字符").max(128, "密码最多 128 个字符"),
		confirmPassword: z.string(),
	})
	.refine((values) => values.password === values.confirmPassword, {
		message: "两次输入的密码不一致",
		path: ["confirmPassword"],
	});

type LoginValues = z.infer<typeof loginSchema>;
type InitValues = z.infer<typeof initSchema>;

export default function LoginPage() {
	const location = useLocation();
	const navigate = useNavigate();
	const { toastError } = useToastActions();
	const from = (location.state as { from?: string } | null)?.from ?? "/";

	const { data: me, isLoading: meLoading } = useMe();
	const { data: status, isLoading: statusLoading } = useAuthStatus();
	const login = useLogin();
	const initAdmin = useInitAdmin();

	// 已登录用户访问登录页时直接回跳。
	if (me) {
		return <Navigate to={from} replace />;
	}

	const loading = meLoading || statusLoading || status === undefined;
	const initMode = status?.initialized === false;

	const submitCredentials = (
		values: { username: string; password: string },
		action: ReturnType<typeof useLogin> | ReturnType<typeof useInitAdmin>,
	) => {
		action.mutate(values, {
			onSuccess: () => navigate(from, { replace: true }),
			onError: (error) => toastError(initMode ? "初始化失败" : "登录失败", error),
		});
	};

	return (
		<div className="flex min-h-screen flex-1 items-center justify-center p-6">
			<div className="content-surface w-full max-w-sm rounded-3xl p-8 shadow-[0_8px_30px_rgba(15,23,42,0.08)]">
				<div className="mb-6 flex flex-col items-center gap-3 text-center">
					<div className="flex size-12 items-center justify-center rounded-2xl bg-foreground text-background shadow-[inset_0_1px_0_rgba(255,255,255,0.15),0_4px_10px_rgba(15,23,42,0.18)] dark:bg-primary dark:text-primary-foreground">
						<Settings className="size-6" />
					</div>
					<div>
						<h1 className="text-xl font-bold tracking-tight">
							{initMode ? "初始化管理员" : "登录 LLM Gateway"}
						</h1>
						<p className="mt-1 text-sm text-muted-foreground">
							{initMode ? "首次使用，请创建管理员账号" : "请输入用户名和密码进入管理后台"}
						</p>
					</div>
				</div>

				{loading ? (
					<div className="py-8 text-center text-sm text-muted-foreground" aria-busy="true">
						加载中...
					</div>
				) : initMode ? (
					<InitForm
						loading={initAdmin.isPending}
						onSubmit={(values) => submitCredentials(values, initAdmin)}
					/>
				) : (
					<LoginForm
						loading={login.isPending}
						onSubmit={(values) => submitCredentials(values, login)}
					/>
				)}
			</div>
		</div>
	);
}

function LoginForm({
	loading,
	onSubmit,
}: {
	loading: boolean;
	onSubmit: (values: { username: string; password: string }) => void;
}) {
	const form = useForm<LoginValues>({
		resolver: zodResolver(loginSchema),
		defaultValues: { username: "", password: "" },
	});

	return (
		<form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
			<div className="grid gap-1.5">
				<Label htmlFor="login-username">用户名</Label>
				<Input id="login-username" autoComplete="username" {...form.register("username")} />
				{form.formState.errors.username && (
					<p className="text-sm text-destructive">{form.formState.errors.username.message}</p>
				)}
			</div>
			<div className="grid gap-1.5">
				<Label htmlFor="login-password">密码</Label>
				<Input
					id="login-password"
					type="password"
					autoComplete="current-password"
					{...form.register("password")}
				/>
				{form.formState.errors.password && (
					<p className="text-sm text-destructive">{form.formState.errors.password.message}</p>
				)}
			</div>
			<Button type="submit" className="mt-2 w-full" disabled={loading}>
				登录
			</Button>
		</form>
	);
}

function InitForm({
	loading,
	onSubmit,
}: {
	loading: boolean;
	onSubmit: (values: { username: string; password: string }) => void;
}) {
	const form = useForm<InitValues>({
		resolver: zodResolver(initSchema),
		defaultValues: { username: "", password: "", confirmPassword: "" },
	});

	const handleSubmit = (values: InitValues) => {
		onSubmit({ username: values.username, password: values.password });
	};

	return (
		<form onSubmit={form.handleSubmit(handleSubmit)} className="grid gap-4">
			<div className="grid gap-1.5">
				<Label htmlFor="init-username">用户名</Label>
				<Input id="init-username" autoComplete="username" {...form.register("username")} />
				{form.formState.errors.username && (
					<p className="text-sm text-destructive">{form.formState.errors.username.message}</p>
				)}
			</div>
			<div className="grid gap-1.5">
				<Label htmlFor="init-password">密码</Label>
				<Input
					id="init-password"
					type="password"
					autoComplete="new-password"
					{...form.register("password")}
				/>
				{form.formState.errors.password && (
					<p className="text-sm text-destructive">{form.formState.errors.password.message}</p>
				)}
			</div>
			<div className="grid gap-1.5">
				<Label htmlFor="init-confirm-password">确认密码</Label>
				<Input
					id="init-confirm-password"
					type="password"
					autoComplete="new-password"
					{...form.register("confirmPassword")}
				/>
				{form.formState.errors.confirmPassword && (
					<p className="text-sm text-destructive">
						{form.formState.errors.confirmPassword.message}
					</p>
				)}
			</div>
			<Button type="submit" className="mt-2 w-full" disabled={loading}>
				<ShieldCheck className="size-4" />
				创建管理员
			</Button>
		</form>
	);
}
