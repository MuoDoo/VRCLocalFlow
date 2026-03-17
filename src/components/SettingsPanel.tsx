import { AudioSelector } from "./AudioSelector";
import { AudioDevice } from "../hooks/useAudioDevices";

export interface WhisperModel {
  id: string;
  name: string;
  filename: string;
  size_mb: number;
  downloaded: boolean;
}

export interface TranslationModelInfo {
  id: string;
  name: string;
  size_mb: number;
  downloaded: boolean;
}

export interface BackendInfo {
  id: string;
  name: string;
  available: boolean;
}

const BASE_LANGUAGES = [
  { code: "en", name: "English" },
  { code: "zh", name: "Chinese" },
  { code: "ja", name: "Japanese" },
];

interface SettingsPanelProps {
  open: boolean;
  onClose: () => void;
  devices: AudioDevice[];
  devicesLoading: boolean;
  onRefreshDevices: () => void;
  selectedDevice: string;
  onSelectDevice: (id: string) => void;
  sourceLang: string;
  targetLang: string;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onSwapLanguages: () => void;
  ttsEnabled: boolean;
  onTtsToggle: (enabled: boolean) => void;
  outputDevices: { name: string }[];
  selectedOutputDevice: string;
  onSelectOutputDevice: (name: string) => void;
  modelPath: string;
  onModelPathChange: (path: string) => void;
  models: WhisperModel[];
  downloadingModel: string | null;
  downloadProgress: number;
  onDownloadModel: (modelId: string) => void;
  requiredTranslationModels: TranslationModelInfo[];
  downloadingTranslationModel: string | null;
  translationDownloadProgress: number;
  onDownloadTranslationModel: (modelId: string) => void;
  vrchatOscEnabled: boolean;
  onVrchatOscToggle: (enabled: boolean) => void;
  vrchatOscPort: number;
  onVrchatOscPortChange: (port: number) => void;
  backends: BackendInfo[];
  selectedBackend: string;
  onBackendChange: (backend: string) => void;
}

function formatSize(mb: number): string {
  if (mb >= 1000) return `${(mb / 1000).toFixed(1)} GB`;
  return `${mb} MB`;
}

export function SettingsPanel({
  open,
  onClose,
  devices,
  devicesLoading,
  onRefreshDevices,
  selectedDevice,
  onSelectDevice,
  sourceLang,
  targetLang,
  onSourceLangChange,
  onTargetLangChange,
  onSwapLanguages,
  ttsEnabled,
  onTtsToggle,
  outputDevices,
  selectedOutputDevice,
  onSelectOutputDevice,
  modelPath,
  onModelPathChange,
  models,
  downloadingModel,
  downloadProgress,
  onDownloadModel,
  requiredTranslationModels,
  downloadingTranslationModel,
  translationDownloadProgress,
  onDownloadTranslationModel,
  vrchatOscEnabled,
  onVrchatOscToggle,
  vrchatOscPort,
  onVrchatOscPortChange,
  backends,
  selectedBackend,
  onBackendChange,
}: SettingsPanelProps) {
  const languages = BASE_LANGUAGES;
  return (
    <>
      {/* Backdrop */}
      {open && (
        <div
          className="fixed inset-0 bg-black/40 z-40"
          onClick={onClose}
        />
      )}

      {/* Drawer */}
      <div
        className={`fixed top-0 right-0 h-full w-[85vw] max-w-sm bg-gray-800 border-l border-gray-700 z-50 transform transition-transform duration-200 ${
          open ? "translate-x-0" : "translate-x-full"
        }`}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-700">
          <h2 className="text-white font-semibold">Settings</h2>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-white text-xl leading-none"
          >
            &times;
          </button>
        </div>

        <div className="px-4 py-4 space-y-5 overflow-y-auto h-[calc(100%-3.5rem)]">
          <AudioSelector
            devices={devices}
            selected={selectedDevice}
            onChange={onSelectDevice}
            loading={devicesLoading}
            onRefresh={onRefreshDevices}
          />

          {/* Engine Backend */}
          {backends.length > 0 && (
            <div className="space-y-2">
              <label className="text-xs text-gray-400 uppercase tracking-wide">
                Engine Backend
              </label>
              <div className="space-y-1">
                {backends.map((b) => {
                  const isSelected = selectedBackend === b.id;
                  return (
                    <div
                      key={b.id}
                      className={`flex items-center gap-2 px-3 py-2 rounded text-sm transition-colors ${
                        isSelected
                          ? "bg-blue-600/30 border border-blue-500/50"
                          : b.available
                          ? "bg-gray-700 hover:bg-gray-600 border border-transparent cursor-pointer"
                          : "bg-gray-700/50 border border-transparent opacity-50"
                      }`}
                      onClick={() => {
                        if (b.available) {
                          onBackendChange(b.id);
                        }
                      }}
                    >
                      <span className="w-4 text-center flex-shrink-0">
                        {isSelected && b.available && (
                          <span className="text-blue-400">&#10003;</span>
                        )}
                      </span>
                      <span className="text-white flex-1">{b.name}</span>
                      {!b.available && (
                        <span className="text-gray-500 text-xs">Not installed</span>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* Language Pair */}
          <div className="space-y-2">
            <label className="text-xs text-gray-400 uppercase tracking-wide">
              Source Language
            </label>
            <select
              className="w-full bg-gray-700 text-white text-sm rounded px-3 py-2 outline-none focus:ring-1 focus:ring-blue-500"
              value={sourceLang}
              onChange={(e) => onSourceLangChange(e.target.value)}
            >
              {languages.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.name}
                </option>
              ))}
            </select>

            <button
              onClick={onSwapLanguages}
              className="w-full text-sm rounded px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-gray-300 transition-colors"
              title="Swap languages"
            >
              &#8645; Swap
            </button>

            <label className="text-xs text-gray-400 uppercase tracking-wide">
              Target Language
            </label>
            <select
              className="w-full bg-gray-700 text-white text-sm rounded px-3 py-2 outline-none focus:ring-1 focus:ring-blue-500"
              value={targetLang}
              onChange={(e) => onTargetLangChange(e.target.value)}
            >
              {languages.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.name}
                </option>
              ))}
            </select>
          </div>

          {/* Translation Model Status */}
          {requiredTranslationModels.length > 0 && (
            <div className="space-y-2">
              <label className="text-xs text-gray-400 uppercase tracking-wide">
                Translation Model
              </label>
              <div className="space-y-1">
                {requiredTranslationModels.map((model) => {
                  const isDownloading = downloadingTranslationModel === model.id;
                  return (
                    <div
                      key={model.id}
                      className="flex items-center gap-2 px-3 py-2 rounded text-sm bg-gray-700 border border-transparent"
                    >
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-white">{model.name}</span>
                          <span className="text-gray-500 text-xs">
                            {formatSize(model.size_mb)}
                          </span>
                        </div>
                        {isDownloading && (
                          <div className="mt-1">
                            <div className="w-full bg-gray-600 rounded-full h-1.5">
                              <div
                                className="bg-blue-500 h-1.5 rounded-full transition-all duration-300"
                                style={{ width: `${translationDownloadProgress}%` }}
                              />
                            </div>
                            <span className="text-xs text-gray-400 mt-0.5">
                              {translationDownloadProgress}%
                            </span>
                          </div>
                        )}
                      </div>
                      <div className="flex-shrink-0">
                        {model.downloaded ? (
                          <span className="text-green-400 text-xs">&#10003;</span>
                        ) : isDownloading ? (
                          <span className="text-blue-400 text-xs animate-pulse">
                            ...
                          </span>
                        ) : (
                          <button
                            onClick={() => onDownloadTranslationModel(model.id)}
                            className="text-xs px-2 py-0.5 rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors"
                          >
                            Download
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* TTS toggle */}
          <div className="space-y-1">
            <label className="text-xs text-gray-400 uppercase tracking-wide">
              Text-to-Speech
            </label>
            <button
              onClick={() => onTtsToggle(!ttsEnabled)}
              className={`w-full text-sm rounded px-3 py-2 transition-colors ${
                ttsEnabled
                  ? "bg-blue-600 hover:bg-blue-500 text-white"
                  : "bg-gray-700 hover:bg-gray-600 text-gray-300"
              }`}
            >
              {ttsEnabled ? "Enabled" : "Disabled"}
            </button>
          </div>

          {/* TTS Output Device */}
          {ttsEnabled && (
            <div className="space-y-1">
              <label className="text-xs text-gray-400 uppercase tracking-wide">
                TTS Output Device
              </label>
              <select
                className="w-full bg-gray-700 text-white text-sm rounded px-3 py-2 outline-none focus:ring-1 focus:ring-blue-500"
                value={selectedOutputDevice}
                onChange={(e) => onSelectOutputDevice(e.target.value)}
              >
                <option value="">None (disabled)</option>
                {outputDevices.map((d) => (
                  <option key={d.name} value={d.name}>
                    {d.name}
                  </option>
                ))}
              </select>
            </div>
          )}

          {/* VRChat OSC */}
          <div className="space-y-1">
            <label className="text-xs text-gray-400 uppercase tracking-wide">
              VRChat OSC
            </label>
            <button
              onClick={() => onVrchatOscToggle(!vrchatOscEnabled)}
              className={`w-full text-sm rounded px-3 py-2 transition-colors ${
                vrchatOscEnabled
                  ? "bg-blue-600 hover:bg-blue-500 text-white"
                  : "bg-gray-700 hover:bg-gray-600 text-gray-300"
              }`}
            >
              {vrchatOscEnabled ? "Enabled" : "Disabled"}
            </button>
          </div>

          {/* VRChat OSC Port */}
          {vrchatOscEnabled && (
            <div className="space-y-1">
              <label className="text-xs text-gray-400 uppercase tracking-wide">
                OSC Port
              </label>
              <input
                type="number"
                className="w-full bg-gray-700 text-white text-sm rounded px-3 py-2 outline-none focus:ring-1 focus:ring-blue-500"
                value={vrchatOscPort}
                onChange={(e) => {
                  const val = parseInt(e.target.value, 10);
                  if (!isNaN(val) && val > 0 && val <= 65535) {
                    onVrchatOscPortChange(val);
                  }
                }}
                min={1}
                max={65535}
              />
            </div>
          )}

          {/* Whisper Model */}
          <div className="space-y-2">
              <label className="text-xs text-gray-400 uppercase tracking-wide">
                Whisper Model
              </label>
              <div className="space-y-1">
                {models.map((model) => {
                  const isSelected = modelPath === model.id;
                  const isDownloading = downloadingModel === model.id;

                  return (
                    <div
                      key={model.id}
                      className={`flex items-center gap-2 px-3 py-2 rounded text-sm cursor-pointer transition-colors ${
                        isSelected
                          ? "bg-blue-600/30 border border-blue-500/50"
                          : "bg-gray-700 hover:bg-gray-600 border border-transparent"
                      }`}
                      onClick={() => {
                        if (model.downloaded) {
                          onModelPathChange(model.id);
                        }
                      }}
                    >
                      {/* Selection indicator */}
                      <span className="w-4 text-center flex-shrink-0">
                        {isSelected && model.downloaded && (
                          <span className="text-blue-400">&#10003;</span>
                        )}
                      </span>

                      {/* Model info */}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-white">{model.name}</span>
                          <span className="text-gray-500 text-xs">
                            {formatSize(model.size_mb)}
                          </span>
                        </div>
                        {isDownloading && (
                          <div className="mt-1">
                            <div className="w-full bg-gray-600 rounded-full h-1.5">
                              <div
                                className="bg-blue-500 h-1.5 rounded-full transition-all duration-300"
                                style={{ width: `${downloadProgress}%` }}
                              />
                            </div>
                            <span className="text-xs text-gray-400 mt-0.5">
                              {downloadProgress}%
                            </span>
                          </div>
                        )}
                      </div>

                      {/* Download / status */}
                      <div className="flex-shrink-0">
                        {model.downloaded ? (
                          <span className="text-green-400 text-xs">&#10003;</span>
                        ) : isDownloading ? (
                          <span className="text-blue-400 text-xs animate-pulse">
                            ...
                          </span>
                        ) : (
                          <button
                            onClick={(e) => {
                              e.stopPropagation();
                              onDownloadModel(model.id);
                            }}
                            className="text-xs px-2 py-0.5 rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors"
                          >
                            Download
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
        </div>
      </div>
    </>
  );
}
