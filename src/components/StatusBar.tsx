interface StatusBarProps {
  running: boolean;
  error: string | null;
  message?: string | null;
}

export function StatusBar({ running, error, message }: StatusBarProps) {
  return (
    <div className="flex items-center justify-between px-4 py-2 bg-gray-800 border-t border-gray-700 text-xs">
      <div className="flex items-center gap-2">
        <span
          className={`inline-block w-2 h-2 rounded-full ${
            error ? "bg-red-400" : running ? "bg-green-400" : "bg-gray-500"
          }`}
        />
        <span className="text-gray-300">
          {error
            ? "Error"
            : running
            ? message || "Pipeline running"
            : "Pipeline stopped"}
        </span>
      </div>
      {error && <span className="text-red-400 truncate max-w-md">{error}</span>}
    </div>
  );
}
