import { AudioDevice } from "../hooks/useAudioDevices";

interface AudioSelectorProps {
  devices: AudioDevice[];
  selected: string;
  onChange: (deviceId: string) => void;
  loading: boolean;
  onRefresh: () => void;
}

export function AudioSelector({
  devices,
  selected,
  onChange,
  loading,
  onRefresh,
}: AudioSelectorProps) {
  return (
    <div className="space-y-1">
      <label className="text-xs text-gray-400 uppercase tracking-wide">
        Audio Input
      </label>
      <div className="flex gap-2">
        <select
          className="flex-1 bg-gray-700 text-white text-sm rounded px-3 py-2 outline-none focus:ring-1 focus:ring-blue-500"
          value={selected}
          onChange={(e) => onChange(e.target.value)}
          disabled={loading}
        >
          {devices.length === 0 && (
            <option value="">
              {loading ? "Loading..." : "No devices found"}
            </option>
          )}
          {devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
            </option>
          ))}
        </select>
        <button
          onClick={onRefresh}
          disabled={loading}
          className="bg-gray-700 hover:bg-gray-600 text-white text-sm rounded px-3 py-2 disabled:opacity-50"
          title="Refresh devices"
        >
          &#x21bb;
        </button>
      </div>
    </div>
  );
}
