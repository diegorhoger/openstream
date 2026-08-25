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
  | 'studio.error.unknown';

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
  'studio.error.unknown': 'The change was rejected by validation.',
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
  'studio.error.unknown': 'A alteração foi rejeitada pela validação.',
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
