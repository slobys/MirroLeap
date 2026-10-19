import type { ScrcpyConfig } from '../hooks/useScrcpy';
import { MIRROR_PRESETS } from './mirrorPresets';

/** Video and window preferences only; never copy identities, paths or recording state. */
export const DEVICE_PROFILE_FIELDS = [
    'res', 'bitrate', 'fps', 'codec', 'hardwareDecoder', 'renderDriver',
    'audioEnabled', 'deviceFrame', 'rememberWindowPosition', 'clipboardSync', 'autoQuality',
] as const satisfies readonly (keyof ScrcpyConfig)[];

export type DeviceProfile = Partial<Pick<ScrcpyConfig, typeof DEVICE_PROFILE_FIELDS[number]>>;
export type DeviceProfiles = Record<string, DeviceProfile>;
export const DEVICE_PROFILES_KEY = 'mirroleap_device_profiles_v1';

export function profileFromConfig(config: ScrcpyConfig): DeviceProfile {
    const result: Record<string, unknown> = {};
    for (const key of DEVICE_PROFILE_FIELDS) {
        if (config[key] !== undefined) result[key] = config[key];
    }
    return result as DeviceProfile;
}

export function readDeviceProfiles(raw: string | null): DeviceProfiles {
    if (!raw) return {};
    try {
        const parsed: unknown = JSON.parse(raw);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
        const records: DeviceProfiles = {};
        for (const [serial, candidate] of Object.entries(parsed)) {
            if (!serial || serial === '__proto__' || serial === 'constructor' ||
                !candidate || typeof candidate !== 'object' || Array.isArray(candidate)) continue;
            const data = candidate as Record<string, unknown>;
            const value: DeviceProfile = {};
            if (typeof data.res === 'string' && ['800','1024','1280','1600','1920','2560','3840'].includes(data.res)) value.res = data.res;
            if (typeof data.bitrate === 'number' && Number.isInteger(data.bitrate) && data.bitrate >= 1 && data.bitrate <= 50) value.bitrate = data.bitrate;
            if (typeof data.fps === 'number' && [30,60,90,120].includes(data.fps)) value.fps = data.fps;
            if (typeof data.codec === 'string' && ['h264','h265','av1','vp8','vp9'].includes(data.codec)) value.codec = data.codec;
            if (typeof data.hardwareDecoder === 'string' && ['auto','disabled','d3d11va','vaapi','videotoolbox'].includes(data.hardwareDecoder)) value.hardwareDecoder = data.hardwareDecoder as ScrcpyConfig['hardwareDecoder'];
            if (typeof data.renderDriver === 'string' && ['auto','direct3d','opengl','software','metal'].includes(data.renderDriver)) value.renderDriver = data.renderDriver;
            for (const key of ['audioEnabled','deviceFrame','rememberWindowPosition','clipboardSync','autoQuality'] as const) {
                if (typeof data[key] === 'boolean') value[key] = data[key] as boolean;
            }
            records[serial] = value;
        }
        return records;
    } catch { return {}; }
}

export function applyDeviceProfile(config: ScrcpyConfig, profile: DeviceProfile | undefined, device: string): ScrcpyConfig {
    return { ...config, ...(profile || {}), device };
}

/** Saving the phone found on the CURRENT config (not a captured previous
 * selection) prevents rapid A→B→C switches from assigning A's settings
 * to B. This pure transition works correctly with React's queued updates.
 */
export function transitionDeviceSelection(
    current: ScrcpyConfig, target: string, profiles: DeviceProfiles,
): {config: ScrcpyConfig; profiles: DeviceProfiles} {
    if (!target || current.device === target ||
        ['__proto__', 'constructor', 'prototype'].includes(target)) {
        return {config: current, profiles};
    }
    const previous = current.device;
    const safePrevious = previous && !['__proto__', 'constructor', 'prototype'].includes(previous);
    const updatedProfiles = safePrevious
        ? {...profiles, [previous]: profileFromConfig(current)}
        : profiles;
    return {
        config: selectDeviceConfig(
            current, Object.prototype.hasOwnProperty.call(updatedProfiles, target)
                ? updatedProfiles[target] : undefined,
            target, Boolean(safePrevious),
        ),
        profiles: updatedProfiles,
    };
}

/** A stream restart may finish after its phone is no longer selected.
 * Update only that phone's stored preferences and only if the user has not
 * changed its video settings since the restart began. Never touch another
 * device's active UI config from a stale async completion.
 */
export function rememberCompletedVideoForInactiveDevice(
    profiles: DeviceProfiles, device: string,
    previous: ScrcpyConfig,
    video: Pick<ScrcpyConfig, 'res' | 'bitrate' | 'fps' | 'codec'>,
): DeviceProfiles {
    if (!Object.prototype.hasOwnProperty.call(profiles, device)) return profiles;
    const saved = profiles[device];
    const keys = ['res', 'bitrate', 'fps', 'codec'] as const;
    if (keys.some(key => (saved[key] ?? previous[key]) !== previous[key])) {
        return profiles;
    }
    return {...profiles, [device]: {...saved, ...video}};
}

/** A never-before-seen phone starts from safe defaults, not the last phone's
 * quality/decoder/clipboard policy. First selection on legacy installations
 * retains the existing saved global config for backwards compatibility.
 * Recording always needs explicit consent per newly selected device.
 */
export function selectDeviceConfig(
    config: ScrcpyConfig, profile: DeviceProfile | undefined,
    device: string, switchingFromAnotherPhone: boolean,
): ScrcpyConfig {
    const baseline = switchingFromAnotherPhone && !profile ? {
        ...config,
        ...MIRROR_PRESETS.balanced,
        hardwareDecoder:'auto' as const,
        renderDriver:'auto',
        audioEnabled:true,
        autoQuality:false,
        clipboardSync:true,
    } : config;
    return {...applyDeviceProfile(baseline, profile, device), record:false};
}
