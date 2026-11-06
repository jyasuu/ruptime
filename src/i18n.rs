//! Lightweight compile-time localisation for badges, the badge index page and
//! `/api/monitors` status text.
//!
//! Language resolution order: `?lang=` query parameter → `Accept-Language`
//! header → `default_language` from the config file → English.
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    ZhTw,
    ZhCn,
    Ja,
    Ko,
    Es,
    De,
    Fr,
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    Up,
    Down,
    NotFound,
    BadgesTitle,
    BadgesIntro,
    DetailedBadge,
    SimpleBadge,
    Status,
    MonitorUrl,
    Group,
    Tags,
    DomainExpires,
}

static DEFAULT_LANG: OnceLock<Lang> = OnceLock::new();

/// Sets the process-wide default language (first call wins).
pub fn set_default(code: Option<&str>) {
    if let Some(l) = code.and_then(Lang::parse) {
        let _ = DEFAULT_LANG.set(l);
    }
}

pub fn default_lang() -> Lang {
    DEFAULT_LANG.get().copied().unwrap_or(Lang::En)
}

impl Lang {
    pub const ALL: [Lang; 8] = [
        Lang::En,
        Lang::ZhTw,
        Lang::ZhCn,
        Lang::Ja,
        Lang::Ko,
        Lang::Es,
        Lang::De,
        Lang::Fr,
    ];

    /// BCP-47 code, also usable as the HTML `lang` attribute.
    pub fn code(&self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhTw => "zh-TW",
            Lang::ZhCn => "zh-CN",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Es => "es",
            Lang::De => "de",
            Lang::Fr => "fr",
        }
    }

    /// Parses `en`, `zh_TW`, `zh-Hant`, `ja-JP`, … (case-insensitive).
    pub fn parse(tag: &str) -> Option<Lang> {
        let t = tag.trim().to_lowercase().replace('_', "-");
        let primary = t.split('-').next().unwrap_or("");
        match primary {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            "ko" => Some(Lang::Ko),
            "es" => Some(Lang::Es),
            "de" => Some(Lang::De),
            "fr" => Some(Lang::Fr),
            "zh" => {
                let traditional =
                    t.contains("tw") || t.contains("hk") || t.contains("mo") || t.contains("hant");
                Some(if traditional { Lang::ZhTw } else { Lang::ZhCn })
            }
            _ => None,
        }
    }

    /// Picks the best supported language from an `Accept-Language` header.
    pub fn from_accept_language(header: &str) -> Option<Lang> {
        let mut items: Vec<(f32, usize, &str)> = header
            .split(',')
            .enumerate()
            .filter_map(|(i, part)| {
                let mut it = part.trim().split(';');
                let tag = it.next()?.trim();
                if tag.is_empty() || tag == "*" {
                    return None;
                }
                let q = it
                    .find_map(|p| {
                        p.trim()
                            .strip_prefix("q=")
                            .and_then(|v| v.parse::<f32>().ok())
                    })
                    .unwrap_or(1.0);
                Some((q, i, tag))
            })
            .collect();
        items.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        items
            .into_iter()
            .filter(|(q, _, _)| *q > 0.0)
            .find_map(|(_, _, tag)| Lang::parse(tag))
    }
}

/// `?lang=` → `Accept-Language` → configured default.
pub fn resolve(query_lang: Option<&str>, accept_language: Option<&str>) -> Lang {
    query_lang
        .and_then(Lang::parse)
        .or_else(|| accept_language.and_then(Lang::from_accept_language))
        .unwrap_or_else(default_lang)
}

pub fn t(lang: Lang, msg: Msg) -> &'static str {
    use Lang::*;
    use Msg::*;
    match (msg, lang) {
        (Up, En) => "UP",
        (Up, ZhTw) | (Up, ZhCn) => "正常",
        (Up, Ja) => "稼働中",
        (Up, Ko) => "정상",
        (Up, Es) => "ACTIVO",
        (Up, De) => "ONLINE",
        (Up, Fr) => "EN LIGNE",

        (Down, En) => "DOWN",
        (Down, ZhTw) => "異常",
        (Down, ZhCn) => "异常",
        (Down, Ja) => "停止中",
        (Down, Ko) => "장애",
        (Down, Es) => "CAÍDO",
        (Down, De) => "OFFLINE",
        (Down, Fr) => "HORS LIGNE",

        (NotFound, En) => "NOT FOUND",
        (NotFound, ZhTw) => "找不到",
        (NotFound, ZhCn) => "未找到",
        (NotFound, Ja) => "見つかりません",
        (NotFound, Ko) => "찾을 수 없음",
        (NotFound, Es) => "NO ENCONTRADO",
        (NotFound, De) => "NICHT GEFUNDEN",
        (NotFound, Fr) => "INTROUVABLE",

        (BadgesTitle, En) => "Uptime Monitor Badges",
        (BadgesTitle, ZhTw) => "服務監控徽章",
        (BadgesTitle, ZhCn) => "服务监控徽章",
        (BadgesTitle, Ja) => "稼働監視バッジ",
        (BadgesTitle, Ko) => "가동 모니터 배지",
        (BadgesTitle, Es) => "Insignias del monitor de disponibilidad",
        (BadgesTitle, De) => "Verfügbarkeits-Badges",
        (BadgesTitle, Fr) => "Badges de surveillance",

        (BadgesIntro, En) => "Available SVG badges for all monitored targets:",
        (BadgesIntro, ZhTw) => "所有受監控目標的 SVG 徽章：",
        (BadgesIntro, ZhCn) => "所有受监控目标的 SVG 徽章：",
        (BadgesIntro, Ja) => "監視対象すべての SVG バッジ:",
        (BadgesIntro, Ko) => "모든 모니터링 대상의 SVG 배지:",
        (BadgesIntro, Es) => "Insignias SVG disponibles para todos los objetivos:",
        (BadgesIntro, De) => "Verfügbare SVG-Badges für alle überwachten Ziele:",
        (BadgesIntro, Fr) => "Badges SVG disponibles pour toutes les cibles :",

        (DetailedBadge, En) => "Detailed Badge:",
        (DetailedBadge, ZhTw) => "詳細徽章：",
        (DetailedBadge, ZhCn) => "详细徽章：",
        (DetailedBadge, Ja) => "詳細バッジ:",
        (DetailedBadge, Ko) => "상세 배지:",
        (DetailedBadge, Es) => "Insignia detallada:",
        (DetailedBadge, De) => "Detail-Badge:",
        (DetailedBadge, Fr) => "Badge détaillé :",

        (SimpleBadge, En) => "Simple Badge:",
        (SimpleBadge, ZhTw) => "簡易徽章：",
        (SimpleBadge, ZhCn) => "简易徽章：",
        (SimpleBadge, Ja) => "シンプルバッジ:",
        (SimpleBadge, Ko) => "간단 배지:",
        (SimpleBadge, Es) => "Insignia simple:",
        (SimpleBadge, De) => "Einfaches Badge:",
        (SimpleBadge, Fr) => "Badge simple :",

        (Status, En) => "Status:",
        (Status, ZhTw) | (Status, ZhCn) => "狀態：",
        (Status, Ja) => "状態:",
        (Status, Ko) => "상태:",
        (Status, Es) => "Estado:",
        (Status, De) => "Status:",
        (Status, Fr) => "Statut :",

        (MonitorUrl, En) => "Monitor URL:",
        (MonitorUrl, ZhTw) | (MonitorUrl, ZhCn) => "監控網址：",
        (MonitorUrl, Ja) => "監視 URL:",
        (MonitorUrl, Ko) => "모니터 URL:",
        (MonitorUrl, Es) => "URL del monitor:",
        (MonitorUrl, De) => "Monitor-URL:",
        (MonitorUrl, Fr) => "URL surveillée :",

        (Group, En) => "Group:",
        (Group, ZhTw) | (Group, ZhCn) => "群組：",
        (Group, Ja) => "グループ:",
        (Group, Ko) => "그룹:",
        (Group, Es) => "Grupo:",
        (Group, De) => "Gruppe:",
        (Group, Fr) => "Groupe :",

        (Tags, En) => "Tags:",
        (Tags, ZhTw) | (Tags, ZhCn) => "標籤：",
        (Tags, Ja) => "タグ:",
        (Tags, Ko) => "태그:",
        (Tags, Es) => "Etiquetas:",
        (Tags, De) => "Tags:",
        (Tags, Fr) => "Étiquettes :",

        (DomainExpires, En) => "Domain expires in",
        (DomainExpires, ZhTw) => "網域到期剩餘",
        (DomainExpires, ZhCn) => "域名到期剩余",
        (DomainExpires, Ja) => "ドメイン失効まで",
        (DomainExpires, Ko) => "도메인 만료까지",
        (DomainExpires, Es) => "El dominio caduca en",
        (DomainExpires, De) => "Domain läuft ab in",
        (DomainExpires, Fr) => "Le domaine expire dans",
    }
}

/// "99.5% uptime" in the requested language.
pub fn fmt_uptime(lang: Lang, pct: f32) -> String {
    match lang {
        Lang::En => format!("{:.1}% uptime", pct),
        Lang::ZhTw | Lang::ZhCn => format!("可用率 {:.1}%", pct),
        Lang::Ja => format!("稼働率 {:.1}%", pct),
        Lang::Ko => format!("가동률 {:.1}%", pct),
        Lang::Es => format!("{:.1}% disponibilidad", pct),
        Lang::De => format!("{:.1}% Verfügbarkeit", pct),
        Lang::Fr => format!("{:.1}% disponibilité", pct),
    }
}

/// Approximate rendered width in px at 11px Verdana: wide (CJK / fullwidth)
/// glyphs are about twice as wide as Latin ones. Byte length is wrong for these.
pub fn text_px_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let u = c as u32;
            let wide = matches!(u,
                0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF |
                0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6);
            if wide {
                12
            } else {
                7
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags() {
        assert_eq!(Lang::parse("zh_TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("zh-Hant-TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("zh-CN"), Some(Lang::ZhCn));
        assert_eq!(Lang::parse("zh"), Some(Lang::ZhCn));
        assert_eq!(Lang::parse("JA-jp"), Some(Lang::Ja));
        assert_eq!(Lang::parse("xx"), None);
    }

    #[test]
    fn accept_language_q_values() {
        assert_eq!(
            Lang::from_accept_language("fr;q=0.5, de;q=0.9, en;q=0.1"),
            Some(Lang::De)
        );
        assert_eq!(
            Lang::from_accept_language("xx, zh-TW,zh;q=0.9"),
            Some(Lang::ZhTw)
        );
        assert_eq!(Lang::from_accept_language("*"), None);
        assert_eq!(
            Lang::from_accept_language("de;q=0, en;q=0.2"),
            Some(Lang::En)
        );
    }

    #[test]
    fn resolve_precedence() {
        assert_eq!(resolve(Some("ja"), Some("de")), Lang::Ja);
        assert_eq!(resolve(Some("bogus"), Some("de")), Lang::De);
        assert_eq!(resolve(None, None), Lang::En);
    }

    #[test]
    fn every_language_has_every_message() {
        for l in Lang::ALL {
            for m in [
                Msg::Up,
                Msg::Down,
                Msg::NotFound,
                Msg::BadgesTitle,
                Msg::BadgesIntro,
                Msg::DetailedBadge,
                Msg::SimpleBadge,
                Msg::Status,
                Msg::MonitorUrl,
                Msg::Group,
                Msg::Tags,
                Msg::DomainExpires,
            ] {
                assert!(!t(l, m).is_empty());
            }
        }
    }

    #[test]
    fn cjk_is_wider_than_ascii() {
        assert_eq!(text_px_width("UP"), 14);
        assert_eq!(text_px_width("正常"), 24);
    }
}
