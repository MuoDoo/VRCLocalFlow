import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface AudioDevice {
  name: string;
  id: string;
}

export function useAudioDevices() {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<AudioDevice[]>("list_audio_devices");
      setDevices(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  return { devices, loading, error, refresh };
}
