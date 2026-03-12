import { createRoute, OpenAPIHono } from "@hono/zod-openapi";
import { z } from "zod";

import { db, eq, tables } from "@llmgateway/db";
import { logger } from "@llmgateway/logger";
import {
	fromEmail,
	getResendClient,
	replyToEmail,
} from "@llmgateway/shared/email";

import type { ServerTypes } from "@/vars.js";

export const publicContact = new OpenAPIHono<ServerTypes>();

const contactFormSchema = z.object({
	name: z.string().min(2, "Name must be at least 2 characters"),
	email: z.string().email("Invalid email address"),
	country: z.string().min(1, "Please select a country"),
	size: z.string().min(1, "Please select company size"),
	message: z.string().min(10, "Message must be at least 10 characters"),
	honeypot: z.string().optional(),
	timestamp: z.number().optional(),
});

const contactResponseSchema = z.object({
	success: z.boolean(),
	message: z.string(),
});

const submissionTracker = new Map<
	string,
	{ count: number; firstSubmission: number }
>();

const submissionTrackerCleanup = setInterval(
	() => {
		const now = Date.now();
		const oneHour = 60 * 60 * 1000;
		for (const [key, value] of Array.from(submissionTracker.entries())) {
			if (now - value.firstSubmission > oneHour) {
				submissionTracker.delete(key);
			}
		}
	},
	60 * 60 * 1000,
);
submissionTrackerCleanup.unref?.();

const disposableEmailDomains = [
	"tempmail.com",
	"10minutemail.com",
	"guerrillamail.com",
	"mailinator.com",
	"throwaway.email",
	"temp-mail.org",
	"getairmail.com",
	"trashmail.com",
	"yopmail.com",
];

const spamKeywords = [
	"casino",
	"viagra",
	"cialis",
	"lottery",
	"bitcoin",
	"cryptocurrency",
	"investment opportunity",
	"click here",
	"buy now",
	"limited time",
];

const submitEnterpriseContact = createRoute({
	method: "post",
	path: "/enterprise",
	request: {
		body: {
			content: {
				"application/json": {
					schema: contactFormSchema,
				},
			},
		},
	},
	responses: {
		200: {
			content: {
				"application/json": {
					schema: contactResponseSchema,
				},
			},
			description: "Enterprise contact request handled successfully",
		},
		400: {
			content: {
				"application/json": {
					schema: contactResponseSchema,
				},
			},
			description: "Submission rejected by validation or spam checks",
		},
		429: {
			content: {
				"application/json": {
					schema: contactResponseSchema,
				},
			},
			description: "Submission rejected by rate limiting",
		},
		500: {
			content: {
				"application/json": {
					schema: contactResponseSchema,
				},
			},
			description: "Submission could not be processed",
		},
	},
});

function extractClientIP(c: {
	req: { header: (name: string) => string | undefined };
}) {
	const cfConnectingIP = c.req.header("CF-Connecting-IP");
	if (cfConnectingIP) {
		return cfConnectingIP;
	}

	const xForwardedFor = c.req.header("X-Forwarded-For");
	if (xForwardedFor) {
		return xForwardedFor.split(",")[0]?.trim() ?? null;
	}

	return c.req.header("X-Real-IP") ?? c.req.header("Remote-Addr") ?? null;
}

function checkForSpam(text: string): boolean {
	const lowerText = text.toLowerCase();
	return spamKeywords.some((keyword) => lowerText.includes(keyword));
}

function isDisposableEmail(email: string): boolean {
	const domain = email.split("@")[1]?.toLowerCase();
	return disposableEmailDomains.some((disposable) => domain === disposable);
}

async function checkRateLimit(identifier: string): Promise<boolean> {
	const now = Date.now();
	const limit = 3;
	const window = 60 * 60 * 1000;

	const tracker = submissionTracker.get(identifier);

	if (!tracker) {
		submissionTracker.set(identifier, { count: 1, firstSubmission: now });
		return true;
	}

	if (now - tracker.firstSubmission > window) {
		submissionTracker.set(identifier, { count: 1, firstSubmission: now });
		return true;
	}

	if (tracker.count >= limit) {
		return false;
	}

	tracker.count++;
	return true;
}

function escapeHtml(value: string): string {
	return value
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;")
		.replaceAll("'", "&#39;");
}

async function updateSubmissionStatus(
	submissionId: string,
	status: "rejected" | "delivered" | "delivery_failed",
	rejectionReason?: string,
) {
	await db
		.update(tables.enterpriseContactSubmission)
		.set({
			spamFilterStatus: status,
			rejectionReason: rejectionReason ?? null,
		})
		.where(eq(tables.enterpriseContactSubmission.id, submissionId));
}

publicContact.openapi(submitEnterpriseContact, async (c) => {
	const validatedData = c.req.valid("json");
	const ipAddress = extractClientIP(c);
	const userAgent = c.req.header("User-Agent") ?? null;

	const [submission] = await db
		.insert(tables.enterpriseContactSubmission)
		.values({
			name: validatedData.name,
			email: validatedData.email,
			country: validatedData.country,
			size: validatedData.size,
			message: validatedData.message,
			honeypot: validatedData.honeypot ?? null,
			clientTimestampMs: validatedData.timestamp ?? null,
			ipAddress,
			userAgent,
		})
		.returning({ id: tables.enterpriseContactSubmission.id });

	if (!submission) {
		logger.error("Failed to persist enterprise contact submission");
		return c.json(
			{
				success: false,
				message: "Failed to store submission. Please try again later.",
			},
			500,
		);
	}

	if (validatedData.honeypot && validatedData.honeypot.trim() !== "") {
		await updateSubmissionStatus(submission.id, "rejected", "honeypot");
		return c.json({ success: false, message: "Invalid submission" }, 400);
	}

	if (validatedData.timestamp) {
		const timeTaken = Date.now() - validatedData.timestamp;
		if (timeTaken < 3000) {
			await updateSubmissionStatus(
				submission.id,
				"rejected",
				"submitted_too_fast",
			);
			return c.json(
				{
					success: false,
					message: "Please take your time filling out the form",
				},
				400,
			);
		}
	}

	const rateLimitKey = ipAddress ?? "unknown";
	const canSubmit = await checkRateLimit(rateLimitKey);
	if (!canSubmit) {
		await updateSubmissionStatus(submission.id, "rejected", "rate_limited");
		return c.json(
			{
				success: false,
				message:
					"Too many submissions. Please try again later (max 3 per hour)",
			},
			429,
		);
	}

	if (isDisposableEmail(validatedData.email)) {
		await updateSubmissionStatus(submission.id, "rejected", "disposable_email");
		return c.json(
			{
				success: false,
				message: "Please use a valid company email address",
			},
			400,
		);
	}

	const contentToCheck = `${validatedData.name} ${validatedData.message}`;
	if (checkForSpam(contentToCheck)) {
		await updateSubmissionStatus(submission.id, "rejected", "keyword_spam");
		return c.json(
			{
				success: false,
				message: "Your message contains prohibited content",
			},
			400,
		);
	}

	const resend = getResendClient();
	if (!resend) {
		await updateSubmissionStatus(
			submission.id,
			"delivery_failed",
			"resend_not_configured",
		);
		return c.json(
			{
				success: false,
				message: "Email service is not configured. Please try again later.",
			},
			500,
		);
	}

	const htmlContent = `
		<html>
			<head>
				<style>
					body { font-family: Arial, sans-serif; line-height: 1.6; color: #333; }
					.container { max-width: 600px; margin: 0 auto; padding: 20px; }
					.header { background-color: #f8f9fa; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
					.field { margin-bottom: 15px; }
					.label { font-weight: bold; color: #555; }
					.value { color: #333; margin-top: 5px; }
				</style>
			</head>
			<body>
				<div class="container">
					<div class="header">
						<h2 style="margin: 0; color: #2563eb;">New Enterprise Contact Request</h2>
					</div>

					<div class="field">
						<div class="label">Name:</div>
						<div class="value">${escapeHtml(validatedData.name)}</div>
					</div>

					<div class="field">
						<div class="label">Email:</div>
						<div class="value">${escapeHtml(validatedData.email)}</div>
					</div>

					<div class="field">
						<div class="label">Country:</div>
						<div class="value">${escapeHtml(validatedData.country)}</div>
					</div>

					<div class="field">
						<div class="label">Company Size:</div>
						<div class="value">${escapeHtml(validatedData.size)}</div>
					</div>

					<div class="field">
						<div class="label">Message:</div>
						<div class="value" style="white-space: pre-wrap;">${escapeHtml(validatedData.message)}</div>
					</div>
				</div>
			</body>
		</html>
	`;

	const { error } = await resend.emails.send({
		from: fromEmail,
		to: [replyToEmail],
		replyTo: validatedData.email,
		subject: `Enterprise Contact Request from ${validatedData.name}`,
		html: htmlContent,
	});

	if (error) {
		logger.error(
			"Failed to send enterprise contact email",
			new Error(error.message),
		);
		await updateSubmissionStatus(
			submission.id,
			"delivery_failed",
			"resend_send_failed",
		);
		return c.json(
			{
				success: false,
				message: "Failed to send email. Please try again later.",
			},
			500,
		);
	}

	await updateSubmissionStatus(submission.id, "delivered");
	return c.json({ success: true, message: "Email sent successfully" }, 200);
});
