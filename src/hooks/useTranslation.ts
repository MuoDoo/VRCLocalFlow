import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

export interface SubtitleEntry {
  id: number;
  segmentId: number;
  original: string;
  translated: string;
  timestamp: number;
}

const MAX_ENTRIES = 20;

export function useTranslation(
  asrSegmentEvent = "asr-segment",
  translateEvent = "translate-result",
) {
  const [entries, setEntries] = useState<SubtitleEntry[]>([]);
  const nextId = useRef(0);
  const pendingOriginals = useRef<Map<number, string>>(new Map());

  useEffect(() => {
    const unlistenAsr = listen<{ text: string; segment_id: number }>(
      asrSegmentEvent,
      (event) => {
        const { text, segment_id } = event.payload;
        pendingOriginals.current.set(segment_id, text);

        setEntries((prev) => {
          // Check if we already have an entry for this segment_id (partial update)
          const existingIdx = prev.findIndex(
            (e) => e.segmentId === segment_id,
          );
          if (existingIdx !== -1) {
            const updated = [...prev];
            updated[existingIdx] = {
              ...updated[existingIdx],
              original: text,
            };
            return updated;
          }
          // New segment
          const id = nextId.current++;
          const updated = [
            ...prev,
            {
              id,
              segmentId: segment_id,
              original: text,
              translated: "",
              timestamp: Date.now(),
            },
          ];
          return updated.slice(-MAX_ENTRIES);
        });
      },
    );

    const unlistenTranslate = listen<{
      text: string;
      segment_id: number;
    }>(translateEvent, (event) => {
      const { text, segment_id } = event.payload;
      pendingOriginals.current.delete(segment_id);
      setEntries((prev) => {
        // Match by segment_id
        const idx = prev.findIndex((e) => e.segmentId === segment_id);
        if (idx !== -1) {
          const updated = [...prev];
          updated[idx] = { ...updated[idx], translated: text };
          return updated;
        }
        // Fallback: update the last entry without a translation
        const ridx = [...prev].reverse().findIndex((e) => e.translated === "");
        if (ridx !== -1) {
          const actualIdx = prev.length - 1 - ridx;
          const updated = [...prev];
          updated[actualIdx] = { ...updated[actualIdx], translated: text };
          return updated;
        }
        return prev;
      });
    });

    return () => {
      unlistenAsr.then((f) => f());
      unlistenTranslate.then((f) => f());
    };
  }, [asrSegmentEvent, translateEvent]);

  const clear = () => {
    setEntries([]);
    pendingOriginals.current.clear();
  };

  return { entries, clear };
}
