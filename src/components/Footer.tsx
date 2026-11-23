import { ExternalLink, Github, HeartHandshake } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useState, type MouseEvent } from 'react';
import { useI18n } from '../i18n';

/** Project attribution lives in Settings, not on the everyday mirror screen. */
export default function Footer({ version }: { version: string }) {
    const { locale } = useI18n();
    const zh = locale.startsWith('zh');
    const [linkError, setLinkError] = useState(false);
    const links = [
        { destination: 'mirroleap', name: zh ? 'MirroLeap 源码' : 'MirroLeap source', href: 'https://github.com/slobys/MirroLeap' },
        { destination: 'scrcpy_gui', name: zh ? '原始项目' : 'Original scrcpy-gui', href: 'https://github.com/kil0bit-kb/scrcpy-gui' },
        { destination: 'scrcpy', name: 'scrcpy', href: 'https://github.com/Genymobile/scrcpy' },
    ] as const;
    const openProject = async (
        event: MouseEvent<HTMLAnchorElement>, destination: (typeof links)[number]['destination'],
    ) => {
        if (!('__TAURI_INTERNALS__' in window)) return;
        event.preventDefault();
        setLinkError(false);
        try {
            await invoke('open_official_project_link', { destination });
        } catch (error) {
            setLinkError(true);
            console.warn('[MirroLeap] project browser open failed:', error);
        }
    };
    return (
        <footer className="ml-card flex flex-col items-start gap-3 text-xs">
            <div className="flex gap-2 items-center font-bold">
                <HeartHandshake size={17} className="ml-primary" />
                MirroLeap v{version}
            </div>
            <p className="ml-muted leading-5">
                {zh
                    ? '基于 scrcpy-gui 二次开发，采用 Genymobile/scrcpy 核心。感谢原项目作者。'
                    : 'A scrcpy-gui derivative powered by Genymobile/scrcpy. Thanks to the original authors.'}
            </p>
            <div className="flex flex-wrap items-center gap-4">
                {links.map(link => (
                    <a key={link.destination} href={link.href} target="_blank" rel="noopener noreferrer"
                        onClick={event => void openProject(event, link.destination)}
                        className="ml-footer-link inline-flex items-center gap-1.5">
                        {link.name === 'scrcpy' ? <ExternalLink size={13} /> : <Github size={13} />}
                        {link.name}
                    </a>
                ))}
            </div>
            {linkError && <p role="alert" className="text-xs text-red-500">
                {zh ? '无法打开浏览器，请检查系统的默认浏览器设置。' : 'Could not open the browser. Check the system default browser settings.'}
            </p>}
            <span className="ml-muted text-[11px]">
                {zh ? '授权协议：MIT · 本项目与 Genymobile 无官方隶属关系。' : 'MIT License · Not affiliated with Genymobile.'}
            </span>
        </footer>
    );
}
