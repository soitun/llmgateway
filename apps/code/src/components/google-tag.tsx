import Script from "next/script";

interface GoogleTagProps {
	googleTagId?: string;
}

export function GoogleTag({ googleTagId }: GoogleTagProps) {
	if (!googleTagId) {
		return null;
	}

	return (
		<>
			<Script
				src={`https://www.googletagmanager.com/gtag/js?id=${googleTagId}`}
				strategy="afterInteractive"
			/>
			<Script id="google-tag-init" strategy="afterInteractive">
				{`window.dataLayer = window.dataLayer || [];
function gtag(){dataLayer.push(arguments);}
gtag('js', new Date());
gtag('config', '${googleTagId}');`}
			</Script>
		</>
	);
}
