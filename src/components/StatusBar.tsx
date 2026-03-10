import { invoke } from "@tauri-apps/api/core";

interface StatusBarProps {
  running: boolean;
  error: string | null;
}

export function StatusBar({ running, error }: StatusBarProps) {
  const openLogFile = async () => {
    try {
      await invoke("open_log_file");
    } catch (e) {
      console.error("Failed to open log file:", e);
    }
  };

  return (
    <div className="flex items-center justify-between px-4 py-2 bg-gray-800 border-t border-gray-700 text-xs">
      <div className="flex items-center gap-2">
        <span
          className={`inline-block w-2 h-2 rounded-full ${
            running ? "bg-green-400" : "bg-gray-500"
          }`}
        />
        <span className="text-gray-300">
          {running ? "Pipeline running" : "Pipeline stopped"}
        </span>
      </div>
      <div className="flex items-center gap-2">
        {error && <span className="text-red-400 truncate max-w-xs">{error}</span>}
        <button
          onClick={openLogFile}
          className="text-gray-400 hover:text-gray-200 transition-colors"
          title="Open log file"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            viewBox="0 0 20 20"
            fill="currentColor"
            className="w-4 h-4"
          >
            <path
              fillRule="evenodd"
              d="M3 3.5A1.5 1.5 0 0 1 4.5 2h6.879a1.5 1.5 0 0 1 1.06.44l4.122 4.12A1.5 1.5 0 0 1 17 7.622V16.5a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 16.5v-13ZM13.25 9a.75.75 0 0 1 .75.75v4.5a.75.75 0 0 1-1.5 0v-4.5a.75.75 0 0 1 .75-.75Zm-6.5 2a.75.75 0 0 1 .75.75v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 .75-.75Zm4-1a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5a.75.75 0 0 1 .75-.75Z"
              clipRule="evenodd"
            />
          </svg>
        </button>
      </div>
    </div>
  );
}
