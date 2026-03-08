import { useEffect, useRef } from "react";
import { SubtitleEntry } from "../hooks/useTranslation";

interface SubtitleOverlayProps {
  entries: SubtitleEntry[];
}

export function SubtitleOverlay({ entries }: SubtitleOverlayProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries]);

  return (
    <div className="flex-1 overflow-y-auto px-4 py-3 space-y-3">
      {entries.length === 0 && (
        <div className="flex items-center justify-center h-full text-gray-500 text-sm">
          Subtitles will appear here when the pipeline is running.
        </div>
      )}
      {entries.map((entry) => (
        <div key={entry.id} className="space-y-0.5">
          <p className="text-gray-400 text-sm leading-snug">{entry.original}</p>
          {entry.translated && (
            <p className="text-white text-lg leading-snug font-medium">
              {entry.translated}
            </p>
          )}
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  );
}
