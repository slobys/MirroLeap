import { ExternalLink, Github, HeartHandshake } from 'lucide-react';
import { useI18n } from '../i18n';

/** Project attribution lives in Settings, not on the everyday mirror screen. */
export default function Footer({ version }: { version: string }) {
    const { locale } = useI18n();
    const zh = locale.startsWith('zh');
    const links = [
        { name: zh ? 'MirroLeap 源码' : 'MirroLeap source', href: 'https://github.com/slobys/MirroLeap' },
        { name: zh ? '原始项目' : 'Original scrcpy-gui', href: 'https://github.com/kil0bit-kb/scrcpy-gui' },
        { name: 'scrcpy', href: 'https://github.com/Genymobile/scrcpy' },
    ];
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
                    <a key={link.href} href={link.href} target="_blank" rel="noopener noreferrer"
                        className="ml-footer-link inline-flex items-center gap-1.5">
                        {link.name === 'scrcpy' ? <ExternalLink size={13} /> : <Github size={13} />}
                        {link.name}
                    </a>
                ))}
            </div>
            <span className="ml-muted text-[11px]">
                {zh ? '授权协议：MIT · 本项目与 Genymobile 无官方隶属关系。' : 'MIT License · Not affiliated with Genymobile.'}
            </span>
        </footer>
    );
}
