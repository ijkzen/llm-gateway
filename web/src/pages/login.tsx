import LocaleToggle from "@/components/locale-toggle";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { useAuthStatus, useInitAdmin, useLogin, useMe } from "@/hooks/use-auth";
import { browserTimezone, saveInitSettings, timezoneOptions } from "@/hooks/use-init-settings";
import { useLocale } from "@/hooks/use-locale";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { Settings, ShieldCheck } from "lucide-react";
import { useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { z } from "zod";

function buildLoginSchema(t: (key: string) => string) {
	return z.object({
		username: z.string().min(1, t("login.usernameRequired")),
		password: z.string().min(1, t("login.passwordRequired")),
	});
}

function buildInitSchema(t: (key: string) => string) {
	return z
		.object({
			username: z.string().min(1, t("login.usernameRequired")).max(64, t("login.usernameMax")),
			password: z.string().min(6, t("login.passwordMin")).max(128, t("login.passwordMax")),
			confirmPassword: z.string(),
		})
		.refine((values) => values.password === values.confirmPassword, {
			message: t("login.passwordMismatch"),
			path: ["confirmPassword"],
		});
}

type LoginValues = z.infer<ReturnType<typeof buildLoginSchema>>;
type InitValues = z.infer<ReturnType<typeof buildInitSchema>>;

export default function LoginPage() {
	const { t } = useTranslation();
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
			onError: (error) => toastError(t(initMode ? "login.initFailed" : "login.loginFailed"), error),
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
							{initMode ? t("login.initTitle") : t("login.title")}
						</h1>
						<p className="mt-1 text-sm text-muted-foreground">
							{initMode ? t("login.initSubtitle") : t("login.subtitle")}
						</p>
					</div>
					<div className="flex justify-end self-end">
						<LocaleToggle />
					</div>
				</div>

				{loading ? (
					<div className="py-8 text-center text-sm text-muted-foreground" aria-busy="true">
						{t("common.loading")}
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
	const { t } = useTranslation();
	const loginSchema = useMemo(() => buildLoginSchema(t), [t]);
	const form = useForm<LoginValues>({
		resolver: zodResolver(loginSchema),
		defaultValues: { username: "", password: "" },
	});

	return (
		<form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
			<div className="grid gap-1.5">
				<Label htmlFor="login-username">{t("login.username")}</Label>
				<Input id="login-username" autoComplete="username" {...form.register("username")} />
				{form.formState.errors.username && (
					<p className="text-sm text-destructive">{form.formState.errors.username.message}</p>
				)}
			</div>
			<div className="grid gap-1.5">
				<Label htmlFor="login-password">{t("login.password")}</Label>
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
				{t("login.login")}
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
	const { t } = useTranslation();
	const { toastError } = useToastActions();
	const locale = useLocale((state) => state.locale);
	const [timezone, setTimezone] = useState(browserTimezone());
	const zones = useMemo(() => timezoneOptions(), []);
	const initSchema = useMemo(() => buildInitSchema(t), [t]);

	const form = useForm<InitValues>({
		resolver: zodResolver(initSchema),
		defaultValues: { username: "", password: "", confirmPassword: "" },
	});

	const handleSubmit = (values: InitValues) => {
		onSubmit({ username: values.username, password: values.password });
		// 语言已在顶部切换入口同步到后端；这里把时区写入设置表。
		// init 失败时 PUT 会因未登录被拒，静默即可（登录后可再改）。
		void saveInitSettings(locale, timezone).then(({ ok, failed }) => {
			if (!ok) {
				toastError(t("common.saveFailed"), new Error(failed.join(", ")));
			}
		});
	};

	return (
		<form onSubmit={form.handleSubmit(handleSubmit)} className="grid gap-4">
			<div className="grid gap-1.5">
				<Label htmlFor="init-username">{t("login.username")}</Label>
				<Input id="init-username" autoComplete="username" {...form.register("username")} />
				{form.formState.errors.username && (
					<p className="text-sm text-destructive">{form.formState.errors.username.message}</p>
				)}
			</div>
			<div className="grid gap-1.5">
				<Label htmlFor="init-password">{t("login.password")}</Label>
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
				<Label htmlFor="init-confirm-password">{t("login.confirmPassword")}</Label>
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

			<div className="grid gap-1.5">
				<Label>{t("login.language")}</Label>
				<Select value={locale} onValueChange={() => {}} disabled>
					<SelectTrigger className="w-full">
						<SelectValue />
					</SelectTrigger>
				</Select>
				<p className="text-xs text-muted-foreground">{t("login.languageHint")}</p>
			</div>

			<div className="grid gap-1.5">
				<Label htmlFor="init-timezone">{t("login.timezone")}</Label>
				<Select value={timezone} onValueChange={setTimezone}>
					<SelectTrigger id="init-timezone" className="w-full">
						<SelectValue />
					</SelectTrigger>
					<SelectContent className="max-h-72">
						{zones.map((zone) => (
							<SelectItem key={zone.value} value={zone.value}>
								{zone.label}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
				<p className="text-xs text-muted-foreground">{t("login.timezoneHint")}</p>
			</div>

			<Button type="submit" className="mt-2 w-full" disabled={loading}>
				<ShieldCheck className="size-4" />
				{t("login.createAdmin")}
			</Button>
		</form>
	);
}
