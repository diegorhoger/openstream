/**
 * OpenStream localization resources.
 *
 * DESIGN_SYSTEM.md: "English and Brazilian Portuguese from the first
 * resource catalog; layout remains RTL-ready." Catalogs are flat string
 * maps keyed by dot-separated message keys; no locale embeds direction or
 * formatting logic, so right-to-left locales can be added without schema
 * change and CSS uses logical properties throughout.
 *
 * Placeholders use `{name}` syntax and resolve through {@link formatMessage}.
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
  | 'shell.footer.note'
  | 'studio.loading'
  | 'studio.loadFailed'
  | 'studio.canvas.heading'
  | 'studio.canvas.gridLabel'
  | 'studio.canvas.openPage'
  | 'studio.canvas.addControlHeading'
  | 'studio.canvas.addButton'
  | 'studio.canvas.addToggle'
  | 'studio.canvas.addPageJump'
  | 'studio.canvas.addDisplay'
  | 'studio.toolbar.label'
  | 'studio.toolbar.undo'
  | 'studio.toolbar.redo'
  | 'studio.toolbar.zoomOut'
  | 'studio.toolbar.zoomIn'
  | 'studio.toolbar.zoomReset'
  | 'studio.zoom.level'
  | 'studio.toolbar.newDeck'
  | 'studio.toolbar.newProfile'
  | 'studio.toolbar.language'
  | 'studio.toolbar.language.en'
  | 'studio.toolbar.language.pt'
  | 'studio.pages.heading'
  | 'studio.pages.add'
  | 'studio.pages.moveUp'
  | 'studio.pages.moveDown'
  | 'studio.pages.delete'
  | 'studio.folders.heading'
  | 'studio.folders.root'
  | 'studio.deck.select'
  | 'studio.deck.moveToFolder'
  | 'studio.deck.delete'
  | 'studio.profiles.heading'
  | 'studio.profiles.none'
  | 'studio.profiles.addDeck'
  | 'studio.profiles.moveUp'
  | 'studio.profiles.moveDown'
  | 'studio.profiles.removeDeck'
  | 'studio.profile.delete'
  | 'studio.inspector.heading'
  | 'studio.inspector.nothingSelected'
  | 'studio.inspector.selectedControl'
  | 'studio.inspector.selectedPage'
  | 'studio.inspector.selectedDeck'
  | 'studio.inspector.selectedProfile'
  | 'studio.inspector.labelField'
  | 'studio.inspector.kindField'
  | 'studio.inspector.policyField'
  | 'studio.inspector.enabledField'
  | 'studio.inspector.xField'
  | 'studio.inspector.yField'
  | 'studio.inspector.widthField'
  | 'studio.inspector.heightField'
  | 'studio.inspector.columnsField'
  | 'studio.inspector.rowsField'
  | 'studio.inspector.titleField'
  | 'studio.inspector.folderField'
  | 'studio.inspector.nameField'
  | 'studio.inspector.deleteControl'
  | 'studio.inspector.deletePage'
  | 'studio.inspector.deleteDeck'
  | 'studio.inspector.deleteProfile'
  | 'studio.inspector.noPolicy'
  | 'studio.control.kind.button'
  | 'studio.control.kind.toggle'
  | 'studio.control.kind.page_jump'
  | 'studio.control.kind.variable_display'
  | 'studio.policy.press'
  | 'studio.policy.release'
  | 'studio.policy.hold'
  | 'studio.policy.repeat'
  | 'studio.policy.toggle'
  | 'studio.state.disabled'
  | 'studio.state.liftedSuffix'
  | 'studio.lift.hint'
  | 'studio.announce.selected'
  | 'studio.announce.lifted'
  | 'studio.announce.dropped'
  | 'studio.announce.canceled'
  | 'studio.announce.pageOpened'
  | 'studio.collision.badge'
  | 'studio.collision.description'
  | 'studio.save.saved'
  | 'studio.save.unavailable'
  | 'studio.save.refused'
  | 'studio.error.prefix'
  | 'studio.error.text_out_of_range'
  | 'studio.error.geometry_outside_grid'
  | 'studio.error.limit_exceeded'
  | 'studio.error.ordinal_conflict'
  | 'studio.error.duplicate_control'
  | 'studio.error.duplicate_deck_ref'
  | 'studio.error.policy_not_allowed'
  | 'studio.error.zero_extent'
  | 'studio.error.revision_overflow'
  | 'studio.error.invalid_folder'
  | 'studio.error.invalid_id'
  | 'studio.error.not_found'
  | 'studio.error.deck_deleted'
  | 'studio.error.invalid_hotkey'
  | 'studio.error.invalid_app_identity'
  | 'studio.error.conflicting_switch_rule'
  | 'studio.error.unknown'
  | 'studio.toolbar.mode'
  | 'studio.mode.edit'
  | 'studio.mode.live'
  | 'surface.heading'
  | 'surface.engine.ready'
  | 'surface.engine.unavailable'
  | 'surface.empty'
  | 'surface.pages.label'
  | 'surface.pages.tab'
  | 'surface.phase.pressed'
  | 'surface.phase.armed'
  | 'surface.phase.relayed'
  | 'surface.phase.accepted'
  | 'surface.phase.running'
  | 'surface.phase.succeeded'
  | 'surface.phase.failed'
  | 'surface.phase.cancelled'
  | 'surface.phase.expired'
  | 'surface.phase.outcome_unknown'
  | 'surface.phase.latched'
  | 'surface.key.stateSink'
  | 'surface.arming.group'
  | 'surface.arming.title'
  | 'surface.arming.confirm'
  | 'surface.arming.cancel'
  | 'surface.announce.pressed'
  | 'surface.announce.armed'
  | 'surface.announce.relayed'
  | 'surface.announce.accepted'
  | 'surface.announce.running'
  | 'surface.announce.succeeded'
  | 'surface.announce.failed'
  | 'surface.announce.cancelled'
  | 'surface.announce.expired'
  | 'surface.announce.outcomeUnknown'
  | 'surface.error.binding_absent'
  | 'surface.error.control_disabled'
  | 'surface.error.state_sink_no_interaction'
  | 'surface.error.policy_mismatch'
  | 'surface.error.unknown'
  | 'studio.profiles.rules.heading'
  | 'studio.profiles.rules.none'
  | 'studio.profiles.rules.addHeading'
  | 'studio.profiles.rules.add'
  | 'studio.profiles.rules.kind'
  | 'studio.profiles.rules.kind.hotkey'
  | 'studio.profiles.rules.kind.app_focus'
  | 'studio.profiles.rules.value.hotkey'
  | 'studio.profiles.rules.value.app_focus'
  | 'studio.profiles.rules.remove'
  | 'studio.profiles.rules.enable'
  | 'studio.profiles.rules.disable'
  | 'studio.profiles.rules.disabledBadge'
  | 'surface.switching.heading'
  | 'surface.switching.active'
  | 'surface.switching.inactive'
  | 'surface.switching.mechanisms.label'
  | 'surface.switching.hotkey.name'
  | 'surface.switching.appFocus.name'
  | 'surface.switching.granted'
  | 'surface.switching.notGranted'
  | 'surface.switching.unsupported'
  | 'surface.switching.grant'
  | 'surface.switching.revoke'
  | 'surface.switching.boardConflict'
  | 'surface.switching.issue.registerConflict'
  | 'surface.switching.issue.registerRefused'
  | 'surface.switching.issue.unregisterRefused'
  | 'surface.switching.issue.focusUnreadable'
  | 'surface.switching.issue.unsupported'
  | 'surface.hint';

export type MessageCatalog = Readonly<Record<MessageKey, string>>;

const EN_US: MessageCatalog = {
  'app.title': 'OpenStream',
  'app.tagline': 'The open control surface for live production.',
  'engine.heading': 'Engine',
  'engine.status.label': 'Engine status:',
  'engine.status.notConnected': 'Not connected',
  'engine.body.muted':
    'The local Engine is not connected in this milestone. Deck authoring runs fully offline.',
  'shell.footer.note': 'Local-first workspace. No account, no network surface.',

  'studio.loading': 'Loading your workspace…',
  'studio.loadFailed':
    'The editor could not load the workspace ({token}). The desktop shell bridge may be unavailable.',

  'studio.canvas.heading': 'Deck canvas',
  'studio.canvas.gridLabel': 'Page {index} of {total}: {columns} columns by {rows} rows',
  'studio.canvas.openPage': 'Open page {index}',
  'studio.canvas.addControlHeading': 'Add control',
  'studio.canvas.addButton': 'Add button',
  'studio.canvas.addToggle': 'Add toggle',
  'studio.canvas.addPageJump': 'Add page jump',
  'studio.canvas.addDisplay': 'Add value display',

  'studio.toolbar.label': 'Editor actions',
  'studio.toolbar.undo': 'Undo',
  'studio.toolbar.redo': 'Redo',
  'studio.toolbar.zoomOut': 'Zoom out',
  'studio.toolbar.zoomIn': 'Zoom in',
  'studio.toolbar.zoomReset': 'Reset zoom',
  'studio.zoom.level': 'Zoom {percent}%',
  'studio.toolbar.newDeck': 'New deck',
  'studio.toolbar.newProfile': 'New profile',
  'studio.toolbar.language': 'Language',
  'studio.toolbar.language.en': 'English',
  'studio.toolbar.language.pt': 'Português (Brasil)',

  'studio.pages.heading': 'Pages',
  'studio.pages.add': 'Add page',
  'studio.pages.moveUp': 'Move page {index} up',
  'studio.pages.moveDown': 'Move page {index} down',
  'studio.pages.delete': 'Delete page {index}',

  'studio.folders.heading': 'Decks and folders',
  'studio.folders.root': 'Workspace root',
  'studio.deck.select': 'Edit deck {title}',
  'studio.deck.moveToFolder': 'Move deck {title} to folder',
  'studio.deck.delete': 'Delete deck {title}',

  'studio.profiles.heading': 'Profiles',
  'studio.profiles.none': 'No profiles yet. Create one from the action bar.',
  'studio.profiles.addDeck': 'Add deck to profile {name}',
  'studio.profiles.moveUp': 'Move {deck} up in profile {name}',
  'studio.profiles.moveDown': 'Move {deck} down in profile {name}',
  'studio.profiles.removeDeck': 'Remove {deck} from profile {name}',
  'studio.profile.delete': 'Delete profile {name}',

  'studio.inspector.heading': 'Inspector',
  'studio.inspector.nothingSelected':
    'Select a control on the canvas, or a page, deck, or profile in the side panels to edit its properties here.',
  'studio.inspector.selectedControl': 'Editing control: {label}',
  'studio.inspector.selectedPage': 'Editing page {index}',
  'studio.inspector.selectedDeck': 'Editing deck: {title}',
  'studio.inspector.selectedProfile': 'Editing profile: {name}',
  'studio.inspector.labelField': 'Label',
  'studio.inspector.kindField': 'Kind',
  'studio.inspector.policyField': 'Interaction policy',
  'studio.inspector.enabledField': 'Enabled',
  'studio.inspector.xField': 'Column X',
  'studio.inspector.yField': 'Row Y',
  'studio.inspector.widthField': 'Width (cells)',
  'studio.inspector.heightField': 'Height (cells)',
  'studio.inspector.columnsField': 'Columns',
  'studio.inspector.rowsField': 'Rows',
  'studio.inspector.titleField': 'Title',
  'studio.inspector.folderField': 'Folder path',
  'studio.inspector.nameField': 'Name',
  'studio.inspector.deleteControl': 'Delete control',
  'studio.inspector.deletePage': 'Delete page',
  'studio.inspector.deleteDeck': 'Delete deck',
  'studio.inspector.deleteProfile': 'Delete profile',
  'studio.inspector.noPolicy': 'No interaction (state display)',

  'studio.control.kind.button': 'Button',
  'studio.control.kind.toggle': 'Toggle',
  'studio.control.kind.page_jump': 'Page jump',
  'studio.control.kind.variable_display': 'Value display',

  'studio.policy.press': 'Fire on press',
  'studio.policy.release': 'Fire on release',
  'studio.policy.hold': 'Hold to fire',
  'studio.policy.repeat': 'Repeat while held',
  'studio.policy.toggle': 'Latch on press',
  'studio.state.disabled': 'Disabled',
  'studio.state.liftedSuffix': 'lifted for keyboard move',

  'studio.lift.hint':
    'Arrow keys move the control. Shift plus arrows resize it. Alt plus Shift shrinks. Enter drops it. Escape cancels.',
  'studio.announce.selected': 'Selected {name}.',
  'studio.announce.lifted': 'Lifted {name}. {hint}',
  'studio.announce.dropped': '{name} moved to column {x}, row {y}.',
  'studio.announce.canceled': 'Keyboard move canceled.',
  'studio.announce.pageOpened': 'Page {index} opened.',

  'studio.collision.badge': 'Overlap',
  'studio.collision.description': 'This control overlaps another control on the page.',

  'studio.save.saved': 'All changes saved locally.',
  'studio.save.unavailable':
    'Autosave is unavailable ({token}). Edits stay in memory until persistence recovers.',
  'studio.save.refused': 'Autosave refused this change ({token}). It is kept in memory only.',

  'studio.error.prefix': 'Change refused:',
  'studio.error.text_out_of_range': '{field} must be 1–{max} characters.',
  'studio.error.geometry_outside_grid': 'That would place the control outside the grid ({axis}).',
  'studio.error.limit_exceeded': 'This change would exceed a size limit.',
  'studio.error.ordinal_conflict': 'Two pages would share one position.',
  'studio.error.duplicate_control': 'A control with this identity already exists here.',
  'studio.error.duplicate_deck_ref': 'This deck is already in the profile.',
  'studio.error.policy_not_allowed': 'That interaction policy does not fit this kind.',
  'studio.error.zero_extent': 'Sizes must be at least one cell.',
  'studio.error.revision_overflow': 'This document cannot accept more structural edits.',
  'studio.error.invalid_folder': 'Folder paths use "/"-separated segments without dots or padding.',
  'studio.error.invalid_id': 'An identifier did not match the expected format.',
  'studio.error.not_found': 'The referenced item no longer exists.',
  'studio.error.deck_deleted': 'This deck has been deleted.',
  'studio.error.invalid_hotkey':
    'Use one shortcut key with at least one modifier, like ctrl+shift+f5 (keys: a-z, 0-9, f1-f24).',
  'studio.error.invalid_app_identity':
    'App identities are lowercase file names like obs64.exe (letters, digits, dots, dashes).',
  'studio.error.conflicting_switch_rule':
    'Another profile already binds this exact trigger. Pick a different combination or app.',
  'studio.error.unknown': 'The change was rejected by validation.',

  'studio.toolbar.mode': 'View mode',
  'studio.mode.edit': 'Edit',
  'studio.mode.live': 'Live',

  'surface.heading': 'Live deck',
  'surface.engine.ready': 'Local Engine ready.',
  'surface.engine.unavailable':
    'Local Engine unavailable. Keys stay inert until the shell bridge returns.',
  'surface.empty': 'This page has no controls yet. Add them in Edit mode.',
  'surface.pages.label': 'Pages of this deck',
  'surface.pages.tab': 'Page {index}',
  'surface.phase.pressed': 'Pressed',
  'surface.phase.armed': 'Armed — confirmation required',
  'surface.phase.relayed': 'Sent to Engine',
  'surface.phase.accepted': 'Accepted by Engine',
  'surface.phase.running': 'Running',
  'surface.phase.succeeded': 'Executed',
  'surface.phase.failed': 'Failed',
  'surface.phase.cancelled': 'Cancelled',
  'surface.phase.expired': 'Expired',
  'surface.phase.outcome_unknown': 'Outcome unknown',
  'surface.phase.latched': 'Latched on',
  'surface.key.stateSink': 'Value display: shows a variable value; it takes no input.',
  'surface.arming.group': 'Confirm destructive action',
  'surface.arming.title':
    '{name} is armed. This destructive action runs only after explicit confirmation.',
  'surface.arming.confirm': 'Confirm {name}',
  'surface.arming.cancel': 'Cancel {name}',
  'surface.announce.pressed': '{name} pressed.',
  'surface.announce.armed': '{name} armed. Confirmation required before it fires.',
  'surface.announce.relayed': '{name} sent to the Engine.',
  'surface.announce.accepted': '{name} accepted by the Engine.',
  'surface.announce.running': '{name} running.',
  'surface.announce.succeeded': '{name} executed successfully.',
  'surface.announce.failed': '{name} failed ({token}).',
  'surface.announce.cancelled': '{name} cancelled.',
  'surface.announce.expired': '{name} expired before execution.',
  'surface.announce.outcomeUnknown':
    '{name} outcome unknown. The Engine could not confirm the result; nothing is assumed.',
  'surface.error.binding_absent':
    'No action is bound to this control yet, so nothing was sent. Bind actions in a later update.',
  'surface.error.control_disabled': 'This control is disabled and stays inert.',
  'surface.error.state_sink_no_interaction':
    'Value displays show state; they take no input.',
  'surface.error.policy_mismatch':
    'This gesture does not match the control interaction policy ({event}). Nothing was sent.',
  'surface.error.unknown': 'The request was refused before anything ran ({token}).',
  'studio.profiles.rules.heading': 'Switch rules',
  'studio.profiles.rules.none':
    'No switch rules yet. Add a shortcut or a focused-app trigger to activate this profile automatically.',
  'studio.profiles.rules.addHeading': 'Add switch rule',
  'studio.profiles.rules.add': 'Add rule',
  'studio.profiles.rules.kind': 'Trigger kind',
  'studio.profiles.rules.kind.hotkey': 'Global shortcut',
  'studio.profiles.rules.kind.app_focus': 'When an app has focus',
  'studio.profiles.rules.value.hotkey': 'Combination (e.g. ctrl+shift+f5)',
  'studio.profiles.rules.value.app_focus': 'App identity (e.g. obs64.exe)',
  'studio.profiles.rules.remove': 'Remove rule {trigger}',
  'studio.profiles.rules.enable': 'Enable rule {trigger}',
  'studio.profiles.rules.disable': 'Disable rule {trigger}',
  'studio.profiles.rules.disabledBadge': 'Disabled',
  'surface.switching.heading': 'Profile switching',
  'surface.switching.active': 'Active profile: {name}',
  'surface.switching.inactive': 'No profile is active. Switching happens only through your rules.',
  'surface.switching.mechanisms.label': 'Switching mechanisms',
  'surface.switching.hotkey.name': 'Global shortcuts',
  'surface.switching.appFocus.name': 'Focused-app matching',
  'surface.switching.granted': 'Allowed. Rules of this kind are live.',
  'surface.switching.notGranted': 'Not allowed. Nothing switches until you allow it.',
  'surface.switching.unsupported': 'Unavailable on this platform.',
  'surface.switching.grant': 'Allow {mechanism}',
  'surface.switching.revoke': 'Revoke {mechanism}',
  'surface.switching.boardConflict':
    'Two rules claim the same trigger, so automatic switching is paused. Fix the conflict in the editor.',
  'surface.switching.issue.registerConflict':
    '{combo} is already registered by another application.',
  'surface.switching.issue.registerRefused': 'The system refused registering {combo}.',
  'surface.switching.issue.unregisterRefused': 'The system refused removing {combo}.',
  'surface.switching.issue.focusUnreadable': 'The focused app could not be read just now.',
  'surface.switching.issue.unsupported': 'No backend for this mechanism on {os}.',
  'surface.hint':
    'Space presses and holding Space holds (fires after {hold} ms, repeating every {repeat} ms). Enter taps. Escape cancels arming.',
};

const PT_BR: MessageCatalog = {
  'app.title': 'OpenStream',
  'app.tagline': 'A superfície de controle aberta para produção ao vivo.',
  'engine.heading': 'Engine',
  'engine.status.label': 'Status do Engine:',
  'engine.status.notConnected': 'Não conectado',
  'engine.body.muted':
    'O Engine local não está conectado neste marco. A autoria de decks roda totalmente offline.',
  'shell.footer.note': 'Espaço de trabalho local. Sem conta, sem superfície de rede.',

  'studio.loading': 'Carregando seu espaço de trabalho…',
  'studio.loadFailed':
    'O editor não conseguiu carregar o espaço de trabalho ({token}). A ponte do aplicativo pode estar indisponível.',

  'studio.canvas.heading': 'Tela de deck',
  'studio.canvas.gridLabel': 'Página {index} de {total}: {columns} colunas por {rows} linhas',
  'studio.canvas.openPage': 'Abrir página {index}',
  'studio.canvas.addControlHeading': 'Adicionar controle',
  'studio.canvas.addButton': 'Adicionar botão',
  'studio.canvas.addToggle': 'Adicionar alternador',
  'studio.canvas.addPageJump': 'Adicionar ir para página',
  'studio.canvas.addDisplay': 'Adicionar exibição de valor',

  'studio.toolbar.label': 'Ações do editor',
  'studio.toolbar.undo': 'Desfazer',
  'studio.toolbar.redo': 'Refazer',
  'studio.toolbar.zoomOut': 'Reduzir zoom',
  'studio.toolbar.zoomIn': 'Ampliar zoom',
  'studio.toolbar.zoomReset': 'Redefinir zoom',
  'studio.zoom.level': 'Zoom {percent}%',
  'studio.toolbar.newDeck': 'Novo deck',
  'studio.toolbar.newProfile': 'Novo perfil',
  'studio.toolbar.language': 'Idioma',
  'studio.toolbar.language.en': 'English',
  'studio.toolbar.language.pt': 'Português (Brasil)',

  'studio.pages.heading': 'Páginas',
  'studio.pages.add': 'Adicionar página',
  'studio.pages.moveUp': 'Mover página {index} para cima',
  'studio.pages.moveDown': 'Mover página {index} para baixo',
  'studio.pages.delete': 'Excluir página {index}',

  'studio.folders.heading': 'Decks e pastas',
  'studio.folders.root': 'Raiz do espaço de trabalho',
  'studio.deck.select': 'Editar deck {title}',
  'studio.deck.moveToFolder': 'Mover deck {title} para pasta',
  'studio.deck.delete': 'Excluir deck {title}',

  'studio.profiles.heading': 'Perfis',
  'studio.profiles.none': 'Nenhum perfil ainda. Crie um pela barra de ações.',
  'studio.profiles.addDeck': 'Adicionar deck ao perfil {name}',
  'studio.profiles.moveUp': 'Mover {deck} para cima no perfil {name}',
  'studio.profiles.moveDown': 'Mover {deck} para baixo no perfil {name}',
  'studio.profiles.removeDeck': 'Remover {deck} do perfil {name}',
  'studio.profile.delete': 'Excluir perfil {name}',

  'studio.inspector.heading': 'Inspetor',
  'studio.inspector.nothingSelected':
    'Selecione um controle na tela, ou uma página, deck ou perfil nos painéis laterais para editar suas propriedades aqui.',
  'studio.inspector.selectedControl': 'Editando controle: {label}',
  'studio.inspector.selectedPage': 'Editando página {index}',
  'studio.inspector.selectedDeck': 'Editando deck: {title}',
  'studio.inspector.selectedProfile': 'Editando perfil: {name}',
  'studio.inspector.labelField': 'Rótulo',
  'studio.inspector.kindField': 'Tipo',
  'studio.inspector.policyField': 'Política de interação',
  'studio.inspector.enabledField': 'Ativo',
  'studio.inspector.xField': 'Coluna X',
  'studio.inspector.yField': 'Linha Y',
  'studio.inspector.widthField': 'Largura (células)',
  'studio.inspector.heightField': 'Altura (células)',
  'studio.inspector.columnsField': 'Colunas',
  'studio.inspector.rowsField': 'Linhas',
  'studio.inspector.titleField': 'Título',
  'studio.inspector.folderField': 'Caminho da pasta',
  'studio.inspector.nameField': 'Nome',
  'studio.inspector.deleteControl': 'Excluir controle',
  'studio.inspector.deletePage': 'Excluir página',
  'studio.inspector.deleteDeck': 'Excluir deck',
  'studio.inspector.deleteProfile': 'Excluir perfil',
  'studio.inspector.noPolicy': 'Sem interação (exibição de valor)',

  'studio.control.kind.button': 'Botão',
  'studio.control.kind.toggle': 'Alternador',
  'studio.control.kind.page_jump': 'Ir para página',
  'studio.control.kind.variable_display': 'Exibição de valor',

  'studio.policy.press': 'Dispara ao pressionar',
  'studio.policy.release': 'Dispara ao soltar',
  'studio.policy.hold': 'Segurar para disparar',
  'studio.policy.repeat': 'Repete enquanto segurado',
  'studio.policy.toggle': 'Trava ao pressionar',
  'studio.state.disabled': 'Inativo',
  'studio.state.liftedSuffix': 'levantado para mover pelo teclado',

  'studio.lift.hint':
    'Setas movem o controle. Shift com as setas redimensiona. Alt com Shift encolhe. Enter solta. Escape cancela.',
  'studio.announce.selected': 'Selecionado {name}.',
  'studio.announce.lifted': '{name} levantado. {hint}',
  'studio.announce.dropped': '{name} movido para a coluna {x}, linha {y}.',
  'studio.announce.canceled': 'Movimento pelo teclado cancelado.',
  'studio.announce.pageOpened': 'Página {index} aberta.',

  'studio.collision.badge': 'Sobreposição',
  'studio.collision.description': 'Este controle se sobrepõe a outro controle da página.',

  'studio.save.saved': 'Todas as alterações salvas localmente.',
  'studio.save.unavailable':
    'O salvamento automático está indisponível ({token}). As edições ficam em memória até a persistência se recuperar.',
  'studio.save.refused':
    'O salvamento automático recusou esta alteração ({token}). Ela fica somente em memória.',

  'studio.error.prefix': 'Alteração recusada:',
  'studio.error.text_out_of_range': '{field} deve ter de 1 a {max} caracteres.',
  'studio.error.geometry_outside_grid': 'Isso colocaria o controle fora da grade ({axis}).',
  'studio.error.limit_exceeded': 'Esta alteração excederia um limite de tamanho.',
  'studio.error.ordinal_conflict': 'Duas páginas compartilhariam a mesma posição.',
  'studio.error.duplicate_control': 'Um controle com esta identidade já existe aqui.',
  'studio.error.duplicate_deck_ref': 'Este deck já está no perfil.',
  'studio.error.policy_not_allowed': 'Essa política de interação não serve para este tipo.',
  'studio.error.zero_extent': 'Os tamanhos devem ser de pelo menos uma célula.',
  'studio.error.revision_overflow': 'Este documento não aceita mais edições estruturais.',
  'studio.error.invalid_folder': 'Pastas usam segmentos separados por "/", sem pontos ou espaços.',
  'studio.error.invalid_id': 'Um identificador não correspondeu ao formato esperado.',
  'studio.error.not_found': 'O item referenciado não existe mais.',
  'studio.error.deck_deleted': 'Este deck foi excluído.',
  'studio.error.invalid_hotkey':
    'Use uma tecla com pelo menos um modificador, como ctrl+shift+f5 (teclas: a-z, 0-9, f1-f24).',
  'studio.error.invalid_app_identity':
    'Identidades de aplicativo são nomes de arquivo em minúsculas como obs64.exe (letras, dígitos, pontos, traços).',
  'studio.error.conflicting_switch_rule':
    'Outro perfil já usa exatamente este gatilho. Escolha outra combinação ou aplicativo.',
  'studio.error.unknown': 'A alteração foi rejeitada pela validação.',

  'studio.toolbar.mode': 'Modo de exibição',
  'studio.mode.edit': 'Editar',
  'studio.mode.live': 'Ao vivo',

  'surface.heading': 'Deck ao vivo',
  'surface.engine.ready': 'Engine local pronto.',
  'surface.engine.unavailable':
    'Engine local indisponível. As teclas ficam inertes até a ponte do aplicativo voltar.',
  'surface.empty': 'Esta página ainda não tem controles. Adicione-os no modo Editar.',
  'surface.pages.label': 'Páginas deste deck',
  'surface.pages.tab': 'Página {index}',
  'surface.phase.pressed': 'Pressionado',
  'surface.phase.armed': 'Armado — confirmação necessária',
  'surface.phase.relayed': 'Enviado ao Engine',
  'surface.phase.accepted': 'Aceito pelo Engine',
  'surface.phase.running': 'Executando',
  'surface.phase.succeeded': 'Executado',
  'surface.phase.failed': 'Falhou',
  'surface.phase.cancelled': 'Cancelado',
  'surface.phase.expired': 'Expirado',
  'surface.phase.outcome_unknown': 'Resultado desconhecido',
  'surface.phase.latched': 'Travado ativado',
  'surface.key.stateSink':
    'Exibição de valor: mostra um valor de variável; não recebe entrada.',
  'surface.arming.group': 'Confirmar ação destrutiva',
  'surface.arming.title':
    '{name} está armado. Esta ação destrutiva só executa após confirmação explícita.',
  'surface.arming.confirm': 'Confirmar {name}',
  'surface.arming.cancel': 'Cancelar {name}',
  'surface.announce.pressed': '{name} pressionado.',
  'surface.announce.armed': '{name} armado. Confirmação necessária antes de disparar.',
  'surface.announce.relayed': '{name} enviado ao Engine.',
  'surface.announce.accepted': '{name} aceito pelo Engine.',
  'surface.announce.running': '{name} executando.',
  'surface.announce.succeeded': '{name} executado com sucesso.',
  'surface.announce.failed': '{name} falhou ({token}).',
  'surface.announce.cancelled': '{name} cancelado.',
  'surface.announce.expired': '{name} expirou antes da execução.',
  'surface.announce.outcomeUnknown':
    '{name} com resultado desconhecido. O Engine não pôde confirmar o resultado; nada é presumido.',
  'surface.error.binding_absent':
    'Nenhuma ação está vinculada a este controle ainda, então nada foi enviado. Vincule ações em uma atualização futura.',
  'surface.error.control_disabled': 'Este controle está inativo e permanece inerte.',
  'surface.error.state_sink_no_interaction':
    'Exibições de valor mostram estado; não recebem entrada.',
  'surface.error.policy_mismatch':
    'Esse gesto não corresponde à política de interação do controle ({event}). Nada foi enviado.',
  'surface.error.unknown': 'O pedido foi recusado antes de qualquer execução ({token}).',
  'studio.profiles.rules.heading': 'Regras de troca',
  'studio.profiles.rules.none':
    'Nenhuma regra ainda. Adicione um atalho ou um gatilho de aplicativo para ativar este perfil automaticamente.',
  'studio.profiles.rules.addHeading': 'Adicionar regra de troca',
  'studio.profiles.rules.add': 'Adicionar regra',
  'studio.profiles.rules.kind': 'Tipo de gatilho',
  'studio.profiles.rules.kind.hotkey': 'Atalho global',
  'studio.profiles.rules.kind.app_focus': 'Quando um app tem foco',
  'studio.profiles.rules.value.hotkey': 'Combinação (ex.: ctrl+shift+f5)',
  'studio.profiles.rules.value.app_focus': 'Identidade do app (ex.: obs64.exe)',
  'studio.profiles.rules.remove': 'Remover regra {trigger}',
  'studio.profiles.rules.enable': 'Ativar regra {trigger}',
  'studio.profiles.rules.disable': 'Desativar regra {trigger}',
  'studio.profiles.rules.disabledBadge': 'Inativa',
  'surface.switching.heading': 'Troca de perfis',
  'surface.switching.active': 'Perfil ativo: {name}',
  'surface.switching.inactive':
    'Nenhum perfil ativo. A troca acontece apenas pelas suas regras.',
  'surface.switching.mechanisms.label': 'Mecanismos de troca',
  'surface.switching.hotkey.name': 'Atalhos globais',
  'surface.switching.appFocus.name': 'Detecção de app em foco',
  'surface.switching.granted': 'Permitido. Regras deste tipo estão ativas.',
  'surface.switching.notGranted': 'Não permitido. Nada troca até você permitir.',
  'surface.switching.unsupported': 'Indisponível nesta plataforma.',
  'surface.switching.grant': 'Permitir {mechanism}',
  'surface.switching.revoke': 'Revogar {mechanism}',
  'surface.switching.boardConflict':
    'Duas regras usam o mesmo gatilho, então a troca automática está pausada. Corrija o conflito no editor.',
  'surface.switching.issue.registerConflict':
    '{combo} já está registrada por outro aplicativo.',
  'surface.switching.issue.registerRefused': 'O sistema recusou registrar {combo}.',
  'surface.switching.issue.unregisterRefused': 'O sistema recusou remover {combo}.',
  'surface.switching.issue.focusUnreadable': 'Não foi possível ler o app em foco agora.',
  'surface.switching.issue.unsupported': 'Sem suporte para este mecanismo em {os}.',
  'surface.hint':
    'Espaço pressiona e manter Espaço segurado segura (dispara após {hold} ms, repetindo a cada {repeat} ms). Enter toca. Escape cancela o armar.',
};

export const CATALOG: Readonly<Record<LocaleId, MessageCatalog>> = {
  'en-US': EN_US,
  'pt-BR': PT_BR,
};

export function messagesFor(locale: LocaleId): MessageCatalog {
  return CATALOG[locale];
}

/**
 * Resolves `{name}` placeholders in a template. Unknown placeholders render
 * verbatim so missing data is visible instead of silently dropped.
 */
export function formatMessage(
  template: string,
  params: Readonly<Record<string, string | number>>,
): string {
  return template.replace(/\{([a-z_]+)\}/gi, (match, name: string) => {
    const value = params[name];
    return value === undefined ? match : String(value);
  });
}
