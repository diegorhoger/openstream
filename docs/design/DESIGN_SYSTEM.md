# OpenStream design system

## Direction

**Studio-grade utility.** Dark-first, calm, precise, and high contrast. OpenStream should feel like dependable production equipment, not a neon gamer accessory. Every state must remain legible while a user is looking at a stream rather than the controller.

## Brand

- Name: **OpenStream**
- Descriptor: **The open control surface for live production.**
- Product nouns: Engine, Studio, Surface, Cloud, Mobile.
- Voice: short verbs, observable states, no fake certainty.
- Avoid: “AI-powered” as a brand crutch, jargon, optimistic success before host acknowledgement, and color-only status.

## Color tokens

| Token | Dark | Light | Purpose |
|---|---:|---:|---|
| `canvas` | `#0B0D0E` | `#F4F6F7` | App background |
| `surface-1` | `#14181A` | `#FFFFFF` | Panels/cards |
| `surface-2` | `#1D2326` | `#E9EDEF` | Controls/raised state |
| `border` | `#343C40` | `#C7CED2` | Visible boundaries |
| `text` | `#F5F7F8` | `#111416` | Primary text |
| `text-muted` | `#AAB3B8` | `#4D585E` | Secondary text |
| `signal` | `#2DE2B4` | `#087D65` | Focus, selected, connected |
| `live` | `#FF5D5D` | `#C82632` | Streaming/armed/destructive |
| `warning` | `#FFB020` | `#8A5700` | Degraded/confirmation |
| `info` | `#58A6FF` | `#0969DA` | Informational state |

Color pairs must meet WCAG 2.2 AA; critical text and compact deck labels target AAA where practical. Status always combines color with an icon, label, shape, or motion-independent indicator.

## Typography

- UI: system font stack, falling back to Inter.
- Data/IDs/timing: IBM Plex Mono.
- Scale: 12, 14, 16, 20, 24, 32, 40 px.
- Body default: 16 px; compact secondary text never below 12 px.
- Deck label defaults to two lines with explicit truncation and accessible full label.

## Geometry

- Base spacing unit: 4 px; primary rhythm: 8 px.
- Radii: 6 px controls, 10 px panels, 14 px deck keys.
- Minimum pointer target: 44 x 44 CSS px.
- Default deck key: 88 x 88 px desktop, adaptive on surfaces.
- Focus ring: 3 px `signal` with 2 px canvas separation.
- Drag/drop always has keyboard alternatives.

## Control states

Every control can render: idle, hover, focused, pressed, armed, running, succeeded, failed, disabled, unavailable, and disconnected. “Relayed,” “accepted,” and “executed” are separate states. A transient success animation never hides the final status.

## Motion and sound

- 80–160 ms direct manipulation; 200 ms panel transitions.
- Respect reduced-motion settings and provide a no-animation mode.
- Running may use a restrained progress edge; never rely on flashing.
- Haptic/audio feedback is optional, configurable, and never the only feedback.

## Layout

- Studio: navigation rail, deck canvas, inspector, and collapsible execution timeline.
- Surface: controls dominate; editing chrome is absent.
- Live mode: destructive actions require arming/confirmation; connection and latency remain visible.
- Tablet: multi-panel decks; phone: responsive grid and optional one-button mode.

## Accessibility contract

- Complete keyboard operation and logical focus order.
- Screen-reader name, role, state, shortcut, and outcome for every control.
- VoiceOver/TalkBack and dynamic type support in native clients.
- High-contrast and reduced-motion modes.
- 200% zoom without loss of function.
- No timeout-dependent interaction without extension.
- English and Brazilian Portuguese from the first resource catalog; layout remains RTL-ready.

Accessibility requirements are release criteria, not later polish.
