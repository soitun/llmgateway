declare global {
	interface Window {
		gtag?: (
			command: "config" | "event" | "js" | "set",
			...args: unknown[]
		) => void;
	}
}

export interface SignupConversionOptions {
	email: string;
	method: string;
	sendTo?: string;
}

export function trackSignupConversion({
	email,
	method,
	sendTo,
}: SignupConversionOptions): void {
	if (typeof window === "undefined" || typeof window.gtag !== "function") {
		return;
	}
	window.gtag("set", "user_data", { email });
	window.gtag("event", "sign_up", { method });
	if (sendTo) {
		window.gtag("event", "conversion", { send_to: sendTo });
	}
}
