import { cookies } from "next/headers";
import { NextResponse } from "next/server";

import { getUser } from "@/lib/getUser";

export const dynamic = "force-dynamic";

export async function GET(
	_req: Request,
	{ params }: { params: Promise<{ videoId: string }> },
) {
	const user = await getUser();
	if (!user) {
		return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
	}

	const { videoId } = await params;

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

	const response = await fetch(
		`${gatewayBaseUrl}/v1/videos/${encodeURIComponent(videoId)}`,
		{
			headers: {
				Authorization: `Bearer ${apiKey}`,
				"x-source": "chat.llmgateway.io",
			},
			cache: "no-store",
		},
	);

	const data = await response.json();

	if (!response.ok) {
		return NextResponse.json(
			{ error: data?.message ?? data?.error ?? "Failed to fetch video status" },
			{ status: response.status },
		);
	}

	return NextResponse.json(data, {
		headers: {
			"Cache-Control": "no-store, no-cache, must-revalidate",
		},
	});
}
