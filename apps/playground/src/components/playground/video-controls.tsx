"use client";

import { Loader2, Sparkles } from "lucide-react";
import { useRef } from "react";

import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { getVideoSizeLabel, getVideoSizes } from "@/lib/video-gen";

import type { VideoDuration, VideoSize } from "@/lib/video-gen";

interface VideoControlsProps {
	prompt: string;
	setPrompt: (prompt: string) => void;
	selectedModels: string[];
	videoSize: VideoSize;
	setVideoSize: (value: VideoSize) => void;
	videoDuration: VideoDuration;
	setVideoDuration: (value: VideoDuration) => void;
	isGenerating: boolean;
	onGenerate: () => void;
}

export function VideoControls({
	prompt,
	setPrompt,
	selectedModels,
	videoSize,
	setVideoSize,
	videoDuration,
	setVideoDuration,
	isGenerating,
	onGenerate,
}: VideoControlsProps) {
	const textareaRef = useRef<HTMLTextAreaElement>(null);

	const canGenerate = prompt.trim().length > 0 && selectedModels.length > 0;

	const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
		if (e.key === "Enter" && !e.shiftKey) {
			e.preventDefault();
			if (canGenerate && !isGenerating) {
				onGenerate();
			}
		}
	};

	return (
		<div className="border-b bg-background p-4">
			<div className="max-w-4xl mx-auto space-y-3">
				<div className="rounded-md border-input border dark:bg-input/30 shadow-xs focus-within:ring-1 focus-within:ring-ring">
					<Textarea
						ref={textareaRef}
						value={prompt}
						onChange={(e) => setPrompt(e.target.value)}
						onKeyDown={handleKeyDown}
						placeholder="Describe the video you want to generate..."
						className="min-h-[80px] max-h-[200px] resize-none border-0 bg-transparent dark:bg-transparent focus-visible:ring-0 shadow-none"
						disabled={isGenerating}
					/>
				</div>
				<div className="flex flex-wrap items-center gap-2">
					<Select
						value={videoSize}
						onValueChange={(val) => setVideoSize(val as VideoSize)}
					>
						<SelectTrigger size="sm" className="min-w-[160px]">
							<SelectValue placeholder="Resolution" />
						</SelectTrigger>
						<SelectContent>
							{getVideoSizes().map((size) => (
								<SelectItem key={size} value={size}>
									{getVideoSizeLabel(size)}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					<Select
						value={String(videoDuration)}
						onValueChange={(val) =>
							setVideoDuration(Number(val) as VideoDuration)
						}
					>
						<SelectTrigger size="sm" className="min-w-[100px]">
							<SelectValue placeholder="Duration" />
						</SelectTrigger>
						<SelectContent>
							<SelectItem value="4">4 seconds</SelectItem>
							<SelectItem value="6">6 seconds</SelectItem>
							<SelectItem value="8">8 seconds</SelectItem>
						</SelectContent>
					</Select>
					<div className="flex-1" />
					<Button
						onClick={onGenerate}
						disabled={isGenerating || !canGenerate}
						className="min-w-[120px]"
					>
						{isGenerating ? (
							<>
								<Loader2 className="h-4 w-4 animate-spin mr-2" />
								Generating...
							</>
						) : (
							<>
								<Sparkles className="h-4 w-4 mr-2" />
								Generate
							</>
						)}
					</Button>
				</div>
			</div>
		</div>
	);
}
