import AppLayout from "@/components/layout";
import { RequireAuth } from "@/components/require-auth";
import { Toaster } from "@/components/ui/sonner";
import { useInitTheme } from "@/hooks/use-theme";
import { lazy } from "react";
import { Route, Routes } from "react-router-dom";

const OverviewPage = lazy(() => import("./pages/overview"));
const CronJobsPage = lazy(() => import("./pages/cron-jobs"));
const ProvidersPage = lazy(() => import("./pages/providers"));
const ProviderModelsPage = lazy(() => import("./pages/provider-models"));
const VirtualModelsPage = lazy(() => import("./pages/virtual-models"));
const ApiKeysPage = lazy(() => import("./pages/api-keys"));
const SettingsPage = lazy(() => import("./pages/settings"));
const LoginPage = lazy(() => import("./pages/login"));
const NotFoundPage = lazy(() => import("./pages/not-found"));

function App() {
	useInitTheme();

	return (
		<>
			<Routes>
				<Route
					element={
						<RequireAuth>
							<AppLayout />
						</RequireAuth>
					}
				>
					<Route path="/" element={<OverviewPage />} />
					<Route path="/cron-jobs" element={<CronJobsPage />} />
					<Route path="/providers" element={<ProvidersPage />} />
					<Route path="/provider-models" element={<ProviderModelsPage />} />
					<Route path="/virtual-models" element={<VirtualModelsPage />} />
					<Route path="/api-keys" element={<ApiKeysPage />} />
					<Route path="/settings" element={<SettingsPage />} />
					<Route path="*" element={<NotFoundPage />} />
				</Route>
				<Route path="/login" element={<LoginPage />} />
			</Routes>
			<Toaster />
		</>
	);
}

export default App;
