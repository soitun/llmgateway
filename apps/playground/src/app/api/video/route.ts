import { cookies } from "next/headers";
import { NextResponse } from "next/server";

import { getUser } from "@/lib/getUser";

export const maxDuration = 60;

export async function POST(req: Request) {
	const user = await getUser();
	if (!user) {
		return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
	}

	const cookieStore = await cookies();
	const apiKey =
		cookieStore.get("llmgateway_playground_key")?.value ??
		cookieStore.get("__Host-llmgateway_playground_key")?.value;

	if (!apiKey) {
		return NextResponse.json({ error: "Missing API key" }, { status: 400 });
	}

	const gatewayBaseUrl =
		process.env.GATEWAY_URL?.replace(/\/v1$/, "") ??
		(process.env.NODE_ENV === "development"
			? "http://localhost:4001"
			: "https://api.llmgateway.io");

	const body = await req.json();
	const noFallback = req.headers.get("x-no-fallback");

	const response = await fetch(`${gatewayBaseUrl}/v1/videos`, {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
			Authorization: `Bearer ${apiKey}`,
			"x-source": "chat.llmgateway.io",
			...(noFallback ? { "x-no-fallback": noFallback } : {}),
		},
		body: JSON.stringify(body),
	});

	const data = await response.json();

	if (!response.ok) {
		return NextResponse.json(
			{ error: data?.message ?? data?.error ?? "Video creation failed" },
			{ status: response.status },
		);
	}

	return NextResponse.json(data);
}
