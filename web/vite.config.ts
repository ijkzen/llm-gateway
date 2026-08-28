import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [react()],
	resolve: {
		alias: {
			"@": path.resolve(__dirname, "./src"),
		},
	},
	test: {
		environment: "jsdom",
		globals: true,
		setupFiles: ["./src/test/setup.ts"],
	},
	server: {
		proxy: {
			"/api": {
				target: "http://localhost:4007",
				changeOrigin: true,
			},
		},
	},
	build: {
		outDir: "dist",
		sourcemap: false,
		rollupOptions: {
			output: {
				manualChunks(id) {
					if (id.includes("node_modules/react-router-dom")) return "router";
					if (id.includes("node_modules/@tanstack/react-query")) return "query";
					if (id.includes("node_modules/@radix-ui")) return "ui";
					if (id.includes("node_modules/lucide-react")) return "icons";
					if (id.includes("node_modules/react-dom") || id.includes("node_modules/react/")) {
						return "react-vendor";
					}
				},
			},
		},
	},
});
