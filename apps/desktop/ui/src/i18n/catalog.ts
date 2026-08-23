/**
 * OpenStream localization resource skeleton.
 *
 * DESIGN_SYSTEM.md: "English and Brazilian Portuguese from the first
 * resource catalog; layout remains RTL-ready." Catalogs are flat string
 * maps keyed by dot-separated message keys; no locale embeds direction or
 * formatting logic, so right-to-left locales can be added without schema
 * change and CSS uses logical properties throughout.
 */

export const LOCALES = ['en-US', 'pt-BR'] as const;

export type LocaleId = (typeof LOCALES)[number];

export const DEFAULT_LOCALE: LocaleId = 'en-US';

export type MessageKey =
  | 'app.title'
  | 'app.tagline'
  | 'engine.heading'
  | 'engine.status.label'
  | 'engine.status.notConnected'
  | 'engine.body.muted'
  | 'shell.footer.note';

export type MessageCatalog = Readonly<Record<MessageKey, string>>;

const EN_US: MessageCatalog = {
  'app.title': 'OpenStream',
  'app.tagline': 'The open control surface for live production.',
  'engine.heading': 'Engine',
  'engine.status.label': 'Engine status:',
  'engine.status.notConnected': 'Not connected',
  'engine.body.muted':
    'The local Engine is not wired into this shell yet. Deck editing and device control arrive in later milestones.',
  'shell.footer.note': 'M0 scaffold — no account, no network surface.',
};

const PT_BR: MessageCatalog = {
  'app.title': 'OpenStream',
  'app.tagline': 'A superfície de controle aberta para produção ao vivo.',
  'engine.heading': 'Engine',
  'engine.status.label': 'Status do Engine:',
  'engine.status.notConnected': 'Não conectado',
  'engine.body.muted':
    'O Engine local ainda não está conectado a este shell. Edição de decks e controle de dispositivos chegam em marcos futuros.',
  'shell.footer.note': 'Scaffold M0 — sem conta, sem superfície de rede.',
};

export const CATALOG: Readonly<Record<LocaleId, MessageCatalog>> = {
  'en-US': EN_US,
  'pt-BR': PT_BR,
};

export function messagesFor(locale: LocaleId): MessageCatalog {
  return CATALOG[locale];
}
