import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import type { ScrcpyConfig, RecordingEvent } from '../hooks/useScrcpy';
import Tooltip from './Tooltip';
import { Coffee, Volume2, Circle, Folder, ChevronDown, ActivitySquare } from 'lucide-react';
import { useI18n } from '../i18n';

const AUDIO_CODEC_VALUES = ['auto', 'opus', 'aac', 'flac', 'raw'] as const;
type AudioCodec = typeof AUDIO_CODEC_VALUES[number];

interface SessionBehaviorProps {
    config: ScrcpyConfig;
    setConfig: (c: ScrcpyConfig) => void;
    isRunning?: boolean;
    activeRecording?: { record: boolean; recordPath: string };
    recordingStatus?: RecordingEvent;
    isApplyingRecording?: boolean;
    onApplyRecordingNow?: () => Promise<boolean>;
}

export default function SessionBehavior({
    config, setConfig, isRunning = false, activeRecording, recordingStatus,
    isApplyingRecording = false, onApplyRecordingNow,
}: SessionBehaviorProps) {
    const { t, locale } = useI18n();
    const [folderError, setFolderError] = useState('');
    const [recordingActionError, setRecordingActionError] = useState('');
    const recordingSettingsPending = isRunning && (!activeRecording
        || activeRecording.record !== Boolean(config.record)
        || (Boolean(config.record) && activeRecording.recordPath !== (config.recordPath || '')));

    const handleChange = (field: keyof ScrcpyConfig, value: any) => {
        const newConfig = { ...config, [field]: value };
        setConfig(newConfig);
        if (field === 'recordPath') {
            localStorage.setItem('scrcpy_record_path', value);
        }
    };

    const handlePickFolder = async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
                title: t('sessionBehavior.selectRecordingFolderTitle')
            });
            if (selected && typeof selected === 'string') {
                setFolderError('');
                handleChange('recordPath', selected);
            }
        } catch (e) {
            console.error(e);
            setFolderError(locale.startsWith('zh') ?
                '无法打开文件夹选择窗口，请重试或检查系统权限：' + String(e) :
                'Could not open the folder picker: ' + String(e));
        }
    };

    const AudioCodecPicker = ({ value, onChange, disabled }: { value: AudioCodec, onChange: (v: AudioCodec) => void, disabled: boolean }) => {
        const [isOpen, setIsOpen] = useState(false);
        const ref = useRef<HTMLDivElement>(null);

        useEffect(() => {
            const onDoc = (e: MouseEvent) => {
                if (ref.current && !ref.current.contains(e.target as Node)) setIsOpen(false);
            };
            if (isOpen) document.addEventListener('mousedown', onDoc);
            return () => document.removeEventListener('mousedown', onDoc);
        }, [isOpen]);

        useEffect(() => {
            if (disabled) setIsOpen(false);
        }, [disabled]);

        const labelFor = (v: AudioCodec) =>
            v === 'auto' ? t('sessionBehavior.audioCodecAuto') :
                v === 'opus' ? t('sessionBehavior.audioCodecOpus') :
                    v === 'aac' ? t('sessionBehavior.audioCodecAac') :
                        v === 'flac' ? t('sessionBehavior.audioCodecFlac') :
                            t('sessionBehavior.audioCodecRaw');

        return (
            <div
                ref={ref}
                onClick={(e) => e.stopPropagation()}
                className={`mt-0.5 pl-7 pr-2 pb-1 transition-opacity ${disabled ? 'opacity-40 pointer-events-none' : 'opacity-100'}`}
            >
                <div className="flex items-center gap-1.5">
                    <span className="text-[8px] font-black uppercase text-zinc-500 tracking-widest">{t('sessionBehavior.audioCodec')}</span>
                    <Tooltip text={t('sessionBehavior.audioCodecTooltip')} />
                    <div className="relative ml-auto">
                        <button
                            type="button"
                            onClick={() => !disabled && setIsOpen(o => !o)}
                            disabled={disabled}
                            className="flex items-center gap-1 bg-zinc-950/60 border border-zinc-800 hover:border-primary/60 rounded-md px-1.5 py-0.5 text-[9px] font-bold uppercase tracking-wider text-zinc-300 hover:text-primary transition-colors"
                        >
                            <span>{labelFor(value)}</span>
                            <ChevronDown size={10} className={`transition-transform duration-150 ${isOpen ? 'rotate-180' : ''}`} />
                        </button>
                        {isOpen && (
                            <div className="absolute right-0 top-full mt-1 z-50 min-w-[88px] bg-zinc-950 border border-zinc-800 rounded-md shadow-2xl py-1 backdrop-blur-xl animate-in fade-in zoom-in-95 duration-100">
                                {AUDIO_CODEC_VALUES.map((opt) => (
                                    <div
                                        key={opt}
                                        onClick={() => { onChange(opt); setIsOpen(false); }}
                                        className={`px-2 py-1 text-[9px] uppercase tracking-wider font-bold cursor-pointer transition-colors ${value === opt ? 'bg-primary/20 text-primary' : 'text-zinc-400 hover:bg-primary hover:text-on-primary'}`}
                                    >
                                        {labelFor(opt)}
                                    </div>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            </div>
        );
    };

    const Toggle = ({ checked, onChange, icon: Icon, label, tooltip, danger = false, disabled = false }: { checked: boolean, onChange: (v: boolean) => void, icon: any, label: string, tooltip: string, danger?: boolean, disabled?: boolean }) => (
        <div
            role="switch"
            aria-label={label}
            aria-checked={checked}
            aria-disabled={disabled}
            tabIndex={disabled ? -1 : 0}
            onClick={() => { if (!disabled) onChange(!checked); }}
            onKeyDown={(event) => {
                if (!disabled && (event.key === ' ' || event.key === 'Enter')) {
                    event.preventDefault();
                    onChange(!checked);
                }
            }}
            className={"ml-settings-switch flex items-center justify-between gap-3 group rounded-xl px-3 border border-transparent " +
                (disabled ? "opacity-45 cursor-not-allowed" : "cursor-pointer hover:border-zinc-800")}
        >
            <div className="flex items-center gap-2 min-w-0">
                <div className={`p-2 rounded-lg shrink-0 transition-colors ${checked ? (danger ? 'bg-red-500/10 text-red-500' : 'bg-primary/10 text-primary') : 'bg-zinc-800/50 text-zinc-500 group-hover:text-zinc-300'}`}>
                    <Icon size={16} />
                </div>
                <div className="min-w-0">
                    <div className="flex items-center gap-1.5 min-w-0">
                        <span className={`text-sm font-semibold truncate transition-colors ${checked ? (danger ? 'text-red-400' : 'text-zinc-200') : 'text-zinc-500 group-hover:text-zinc-400'}`}>
                            {label}
                        </span>
                        <div className="shrink-0"><Tooltip text={tooltip} /></div>
                    </div>
                    <p className="ml-setting-hint" title={tooltip}>{tooltip}</p>
                </div>
            </div>
            <div className={`w-10 h-6 shrink-0 rounded-full p-1 transition-all duration-300 ${checked ? (danger ? 'bg-red-600' : 'bg-primary') : 'bg-zinc-800'}`}>
                <div className={`w-4 h-4 rounded-full shadow-sm transition-all duration-300 ${checked ? 'bg-white translate-x-4' : 'bg-white translate-x-0'}`} />
            </div>
        </div>
    );

    return (
        <div className="ml-behavior-groups">
            <section className="ml-card ml-settings-group">
                <div className="ml-settings-group-head">
                    <Coffee size={18} className="ml-primary" />
                    <h2>{locale.startsWith('zh') ? '手机与连接' : 'Device & connection'}</h2>
                </div>

                <div className="flex flex-col gap-1">
                    <Toggle
                        checked={config.stayAwake || false}
                        onChange={(v) => handleChange('stayAwake', v)}
                        icon={Coffee}
                        label={t('sessionBehavior.stayAwake')}
                        tooltip={t('sessionBehavior.stayAwakeTooltip')}
                    />
                    {/* v4: Keep Active */}
                    <Toggle
                        checked={config.keepActive || false}
                        onChange={(v) => handleChange('keepActive', v)}
                        icon={ActivitySquare}
                        label={t('sessionBehavior.keepActive')}
                        tooltip={t('sessionBehavior.keepActiveTooltip')}
                    />
                    <div>
                        <Toggle
                            checked={config.audioEnabled || false}
                            onChange={(v) => handleChange('audioEnabled', v)}
                            icon={Volume2}
                            label={t('sessionBehavior.forwardAudio')}
                            tooltip={t('sessionBehavior.forwardAudioTooltip')}
                        />
                        {config.audioEnabled && (
                            <AudioCodecPicker
                                value={(AUDIO_CODEC_VALUES.includes((config.audioCodec as AudioCodec)) ? config.audioCodec : 'auto') as AudioCodec}
                                onChange={(v) => handleChange('audioCodec', v)}
                                disabled={!config.audioEnabled}
                            />
                        )}
                    </div>
                </div>
            </section>
            <section className="ml-card ml-settings-group ml-behavior-recording">
                <div className="ml-settings-group-head">
                    <Circle size={18} className="ml-primary" />
                    <h2>{locale.startsWith('zh') ? '录制与保存' : 'Recording & saving'}</h2>
                </div>
                <div className="flex flex-col gap-1">
                    <Toggle
                        checked={config.sessionMode === 'mirror' && Boolean(config.record)}
                        disabled={config.sessionMode !== 'mirror'}
                        onChange={(v) => handleChange('record', v)}
                        icon={Circle}
                        label={t('sessionBehavior.recordFeed')}
                        tooltip={t('sessionBehavior.recordFeedTooltip')}
                        danger={true}
                    />
                    <p className="text-xs text-zinc-500 leading-5" role="note">
                        {locale.startsWith('zh')
                            ? '录制来源：Android 手机主屏幕（显示器 0）。不采集 Windows 桌面、手机外框或悬浮工具栏。'
                            : 'Source: Android phone primary screen (display 0). Never captures the Windows desktop, phone bezel or floating toolbar.'}
                    </p>
                    {config.sessionMode !== 'mirror' && (
                        <p className="text-xs text-amber-500" role="status">
                            {locale.startsWith('zh')
                                ? '请先在“投屏参数”切换到“屏幕”模式，才能录制手机自身的屏幕。'
                                : 'Choose the Screen source under Capture before recording the physical phone display.'}
                        </p>
                    )}
                </div>

                <div className="pt-3 border-t border-zinc-800/50 space-y-2">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-1.5">
                            <Folder size={12} className="text-zinc-500" />
                            <span className="text-xs font-semibold text-zinc-500">{t('sessionBehavior.recordPath')}</span>
                        </div>
                        <button
                            onClick={handlePickFolder}
                            disabled={isApplyingRecording}
                            className="text-xs font-semibold text-primary hover:text-white transition-colors disabled:opacity-40"
                        >
                            {t('sessionBehavior.change')}
                        </button>
                    </div>
                    <div className="bg-black/40 border border-zinc-800/50 rounded-lg px-2.5 py-1.5">
                        <p className="text-xs text-zinc-500 font-mono break-all leading-5">
                            {config.recordPath || t('sessionBehavior.defaultVideosFolder')}
                        </p>
                    </div>
                </div>
                {isRunning && recordingSettingsPending && onApplyRecordingNow && (
                    <div className="mt-3 rounded-xl border border-amber-400/30 bg-amber-400/5 p-3 space-y-2" role="status">
                        <p className="text-xs leading-5">
                            {locale.startsWith('zh')
                                ? '录制参数已更改，但 scrcpy 只能在启动时接收录制命令。点击下方按钮会短暂重启投屏；正在录制的文件会先结束并保存。'
                                : 'Recording is configured at stream launch. Apply below to briefly restart mirroring and finalize any current recording first.'}
                        </p>
                        <button type="button"
                            className="ml-secondary-action w-full py-2.5 text-xs font-semibold disabled:opacity-50"
                            disabled={isApplyingRecording}
                            onClick={async () => {
                                setRecordingActionError('');
                                const applied = await onApplyRecordingNow();
                                if (!applied) setRecordingActionError(locale.startsWith('zh')
                                    ? '无法应用录制设置，请查看工具与日志；原有投屏会尽量恢复。'
                                    : 'Could not apply recording settings. Check Tools & Logs.');
                            }}>
                            {isApplyingRecording
                                ? (locale.startsWith('zh') ? '正在保存并重启投屏…' : 'Finalizing and restarting…')
                                : (locale.startsWith('zh') ? '立即应用录制设置并重新开始投屏' : 'Apply recording settings and restart mirroring')}
                        </button>
                    </div>
                )}
                {!isRunning && config.record && (
                    <p className="ml-muted mt-3 text-xs" role="status">
                        {locale.startsWith('zh')
                            ? '已开启录制选项。下次点击“开始投屏”才会开始录制；停止投屏后保存 MKV 文件。'
                            : 'Recording is ready. Start mirroring to record, then stop mirroring to finalize the MKV file.'}
                    </p>
                )}
                {isRunning && !recordingSettingsPending && (
                    <p className="ml-muted mt-3 text-xs" role="status">
                        {activeRecording?.record
                            ? (locale.startsWith('zh') ? '当前会话已启用录制；停止投屏后保存视频。' : 'Current session is recording. Stop mirroring to save.')
                            : (locale.startsWith('zh') ? '当前会话为普通投屏，未启动录制。' : 'This session is not recording.')}
                    </p>
                )}
                {recordingStatus && (
                    <div className="mt-3 rounded-lg border border-zinc-700/60 bg-zinc-900/30 p-3" role="status">
                        <p className="text-xs font-semibold">
                            {recordingStatus.phase === 'preparing' ? (locale.startsWith('zh') ? '正在准备录制…' : 'Preparing recording…')
                            : recordingStatus.phase === 'recording' ? (locale.startsWith('zh') ? '正在录制' : 'Recording')
                            : recordingStatus.phase === 'finalizing' ? (locale.startsWith('zh') ? '正在保存录制…' : 'Saving recording…')
                            : recordingStatus.phase === 'saved' ? (locale.startsWith('zh') ? '录制文件已保存' : 'Recording file saved')
                            : (locale.startsWith('zh') ? '录制保存未确认成功' : 'Recording could not be confirmed')}
                        </p>
                        <p className="text-xs mt-1 break-all opacity-75" title={recordingStatus.path}>
                            {recordingStatus.path}
                        </p>
                        {recordingStatus.phase === 'failed' && (
                            <p className="mt-1 text-xs text-amber-500">{recordingStatus.message}</p>
                        )}
                    </div>
                )}
                {(folderError || recordingActionError) && (
                    <p className="mt-2 text-xs text-red-500" role="alert">
                        {folderError || recordingActionError}
                    </p>
                )}
            </section>
        </div>
    );
}
