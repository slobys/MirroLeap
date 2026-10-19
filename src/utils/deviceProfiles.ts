import type { ScrcpyConfig } from '../hooks/useScrcpy';

/** Video and window preferences only; never copy identities, paths or recording state. */
export const DEVICE_PROFILE_FIELDS = [
    'res', 'bitrate', 'fps', 'codec', 'hardwareDecoder', 'renderDriver',
    'audioEnabled', 'deviceFrame', 'rememberWindowPosition',
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
            if (typeof data.hardwareDecoder === 'string' && ['auto','d3d11va','dxva2','none'].includes(data.hardwareDecoder)) value.hardwareDecoder = data.hardwareDecoder as ScrcpyConfig['hardwareDecoder'];
            if (typeof data.renderDriver === 'string' && ['auto','direct3d','opengl','software','metal'].includes(data.renderDriver)) value.renderDriver = data.renderDriver;
            for (const key of ['audioEnabled','deviceFrame','rememberWindowPosition'] as const) {
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
