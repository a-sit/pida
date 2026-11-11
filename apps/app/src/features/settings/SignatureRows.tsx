/**
 * Einstellungen › Signaturbild (Live-Editor) und › Dokument (ADR 0004 §3.11; Gestalt aus dem Prototyp).
 *
 * Die Vorschau ist die echte Blockvorschau des Kerns (`pida-render://block`, `SettingsView::preview_revision`);
 * nach jeder übernommenen Änderung baut der Kern sie neu. Ohne eigene Bildmarke zeigt der Block das PIDA-Siegel
 * (ADR 0005) – die Oberfläche setzt nie von sich aus eine andere Bildmarke. Eine Bildmarke kommt nur aus einer
 * ausdrücklichen Wahl (Dateidialog, Ablegen auf der Ablagefläche bzw. der Vorschau).
 */
import { useEffect, useRef, useState } from 'react';
import { useApp } from '../../app/context';
import { baseName } from '../../core/defaults';
import type { Lang, NoteSetting, PlaceholderKind, SignatureProfile } from '../../core/types';
import { translator } from '../../i18n/catalog';
import { Button, SwitchField } from '../../ui/controls';
import { IconChevronDown, IconImage, IconPen, IconQr, IconRefresh } from '../../ui/Icons';
import { DefaultEmblem } from '../../ui/Logo';
import { ChoiceCards, ChoiceSegments, LogoSizeSlider, SettingTextField, logoCm } from './controls';
import { useSettings, useTextSetting } from './state';

export const PROFILE_LABEL: Record<SignatureProfile, string> = {
  SIGNATURBLOCK_SMALL: 'ng-ui-set-profile-small',
  AMTSSIGNATURBLOCK: 'ng-ui-set-profile-amt',
  BASE_LOGO: 'ng-ui-set-profile-logo',
  INVISIBLE: 'ng-ui-set-profile-invisible',
};

/** Standardtext des Hinweises je Profil (Katalog von PDF-Over, in der Sprache des Blocks). */
const NOTE_IDS: Partial<Record<SignatureProfile, string>> = {
  SIGNATURBLOCK_SMALL: 'simple_config-Note_Default_Standard',
  AMTSSIGNATURBLOCK: 'simple_config-Note_Default_OfficialSignature',
};

const MORE_PROFILES: SignatureProfile[] = ['BASE_LOGO', 'INVISIBLE'];

type LangChoice = 'system' | Lang;

function useBlockLang(): Lang {
  const { view } = useApp();
  const s = useSettings();
  return s.get<Lang | null>('signature.block_language') ?? view.language;
}

/** Live-Vorschau des Signaturblocks (bleibt beim Bearbeiten oben sichtbar). */
export function BlockPreview() {
  const { bridge, t } = useApp();
  const s = useSettings();
  const profile = s.get<SignatureProfile>('signature.profile');
  const blockLang = s.get<Lang | null>('signature.block_language');
  if (profile === 'INVISIBLE') {
    return (
      <figure className="block-preview">
        <div className="block-preview-stage is-invisible">
          <p className="muted">{t.t('ng-ui-set-preview-invisible')}</p>
        </div>
      </figure>
    );
  }
  const profileName = t.t(PROFILE_LABEL[profile]);
  const alt =
    profile === 'BASE_LOGO'
      ? t.t('ng-ui-set-preview-alt-logo', { profile: profileName })
      : t.t('ng-ui-set-preview-alt', { profile: profileName, lang: t.t(blockLang === 'en' ? 'ng-ui-set-lang-en' : blockLang === 'de' ? 'ng-ui-set-lang-de' : 'ng-ui-set-lang-system') });
  return (
    <figure className="block-preview">
      <div className="block-preview-stage" data-emblem-drop={s.mobile ? undefined : ''}>
        <img className="block-preview-img" src={bridge.blockPreviewUrl(2, s.view.preview_revision)} alt={alt} draggable={false} />
      </div>
      <figcaption className="block-preview-caption">{t.t('ng-ui-set-preview')}</figcaption>
    </figure>
  );
}

export function ProfileRow() {
  const { t } = useApp();
  const s = useSettings();
  const profile = s.get<SignatureProfile>('signature.profile');
  const [more, setMore] = useState(() => MORE_PROFILES.includes(profile));
  const expanded = more || MORE_PROFILES.includes(profile);
  const locked = s.locked('signature.profile');
  const shown: SignatureProfile[] = expanded ? ['SIGNATURBLOCK_SMALL', 'AMTSSIGNATURBLOCK', ...MORE_PROFILES] : ['SIGNATURBLOCK_SMALL', 'AMTSSIGNATURBLOCK'];
  return (
    <div className="stack-3">
      <ChoiceCards<SignatureProfile>
        label={t.t('ng-ui-set-profile')}
        value={profile}
        columns={2}
        isDisabled={!!locked}
        description={locked}
        onChange={(v) => s.apply({ 'signature.profile': v })}
        options={shown.map((p) => ({ value: p, label: t.t(PROFILE_LABEL[p]), description: t.t(`${PROFILE_LABEL[p]}-desc`) }))}
      />
      {MORE_PROFILES.includes(profile) ? null : (
        <Button variant="ghost" size="sm" className="settings-more" aria-expanded={expanded} onPress={() => setMore(!expanded)}>
          <IconChevronDown size={15} className={expanded ? 'is-flipped' : undefined} />
          {t.t('ng-ui-set-profile-more')}
        </Button>
      )}
    </div>
  );
}

export function BlockLangRow() {
  const { t } = useApp();
  const s = useSettings();
  const v = s.get<Lang | null>('signature.block_language');
  return (
    <ChoiceSegments<LangChoice>
      label={t.t('ng-ui-set-block-lang')}
      value={v ?? 'system'}
      onChange={(x) => s.apply({ 'signature.block_language': x === 'system' ? null : x })}
      options={[
        { value: 'system', label: t.t('ng-ui-set-lang-system') },
        { value: 'de', label: t.t('ng-ui-set-lang-de'), lang: 'de' },
        { value: 'en', label: t.t('ng-ui-set-lang-en'), lang: 'en' },
      ]}
    />
  );
}

export const IMAGE_EXTENSIONS = ['png', 'jpg', 'jpeg', 'gif'];

/** Bildmarke: Ablagefläche (Ablegen über die Fenster-Ereignisse der Shell), Dateiwahl, Standard (PIDA-Siegel). */
export function EmblemRow() {
  const { t, bridge, toast, dragActive } = useApp();
  const s = useSettings();
  const emblem = s.get<string | null>('signature.emblem');
  const problem = s.problem('signature.emblem');
  const pick = async () => {
    try {
      const files = await bridge.pickFiles({
        title: t.t('ng-ui-set-emblem-dialog'),
        multiple: false,
        filters: [
          { name: t.t('common-ImageExtension_Description'), extensions: IMAGE_EXTENSIONS },
          { name: t.t('common-AllExtension_Description'), extensions: ['*'] },
        ],
        defaultPath: emblem,
      });
      const file = files?.[0];
      if (file) s.apply({ 'signature.emblem': file });
    } catch {
      toast(t.t('ng-ui-dialog-failed'), 'error');
    }
  };
  return (
    <div className="field">
      <span className="field-label" id="set-emblem-label">
        {t.t('ng-ui-set-emblem')}
      </span>
      <div className={`logo-drop${dragActive ? ' is-target' : ''}`} data-emblem-drop="" role="group" aria-labelledby="set-emblem-label" aria-describedby="set-emblem-hint">
        <div className="logo-drop-inner">
          <span className="logo-thumb" aria-hidden="true">
            {emblem || s.view.emblem_placeholder ? <IconImage size={22} /> : <DefaultEmblem />}
          </span>
          <span className="logo-text">
            <span className="logo-name">
              {emblem
                ? t.t('ng-ui-set-emblem-current', { name: baseName(emblem) })
                : // Amtssignatur ohne eigene Bildmarke: neutraler Platzhalter statt PIDA-Siegel (Backlog Nr. 10, Kern)
                  t.t(s.view.emblem_placeholder ? 'ng-ui-set-emblem-default-amts' : 'ng-ui-set-emblem-default')}
            </span>
            <span className="field-hint" id="set-emblem-hint">
              {t.t('ng-ui-set-emblem-hint')}
            </span>
          </span>
          <span className="logo-actions">
            <Button variant="secondary" size="sm" onPress={() => void pick()}>
              {t.t('ng-ui-set-emblem-choose')}
            </Button>
            {emblem ? (
              <Button variant="ghost" size="sm" onPress={() => s.apply({ 'signature.emblem': null })}>
                {t.t('ng-ui-set-emblem-reset')}
              </Button>
            ) : null}
          </span>
        </div>
      </div>
      {problem ? (
        <p className="field-error" role="alert">
          {problem}
        </p>
      ) : null}
    </div>
  );
}

/** Größe „Nur Bildmarke“ (Regler in cm, gesendet in mm; beim Ziehen entprellt). */
export function LogoSizeRow() {
  const { t } = useApp();
  const s = useSettings();
  const mm = s.get<number>('signature.logo_only_size_mm');
  const [draft, setDraft] = useState<number | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const send = (cm: number) => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    s.apply({ 'signature.logo_only_size_mm': Math.round(cm * 10) });
    setDraft(null);
  };
  const latest = useRef(send);
  latest.current = send;
  // Beim Schließen (z. B. Einstellungen zu, bevor die Entprellung abläuft) den letzten Wert noch senden
  const pending = useRef<number | null>(null);
  useEffect(() => () => {
    if (timer.current && pending.current !== null) latest.current(pending.current);
    else if (timer.current) clearTimeout(timer.current);
  }, []);
  const problem = s.problem('signature.logo_only_size_mm');
  return (
    <div className="stack-2">
      <LogoSizeSlider
        label={t.t('ng-ui-set-logo-size')}
        valueCm={draft ?? logoCm(mm)}
        onChange={(cm) => {
          setDraft(cm);
          pending.current = cm;
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(() => latest.current(cm), 300);
        }}
        onChangeEnd={(cm) => send(cm)}
      />
      {problem ? (
        <p className="field-error" role="alert">
          {problem}
        </p>
      ) : null}
    </div>
  );
}

/** Hinweistext: Standardtext (Katalog in der Blocksprache), eigener Text, leer = kein Hinweis. */
export function NoteRow() {
  const { t } = useApp();
  const s = useSettings();
  const profile = s.get<SignatureProfile>('signature.profile');
  const note = s.get<NoteSetting>('signature.note');
  const lang = useBlockLang();
  const noteId = NOTE_IDS[profile];
  const defaultNote = noteId ? translator(lang).t(noteId) : '';
  const field = useTextSetting(
    'signature.note',
    (v) => {
      const n = v as NoteSetting;
      return n.mode === 'custom' ? n.text : n.mode === 'default' ? defaultNote : '';
    },
    (text) => ({ value: text === defaultNote ? { mode: 'default' } : text.trim() === '' ? { mode: 'none' } : { mode: 'custom', text } }),
  );
  return (
    <div className="note-row">
      <SettingTextField label={t.t('ng-ui-set-note')} multiline description={t.t('ng-ui-set-note-hint')} {...field} />
      {note.mode !== 'default' ? (
        <Button variant="ghost" size="sm" onPress={() => s.apply({ 'signature.note': { mode: 'default' } })}>
          <IconRefresh size={15} />
          {t.t('ng-ui-set-note-reset')}
        </Button>
      ) : null}
    </div>
  );
}

/* ------------------------------------------------------------------ Dokument */

export function PdfaRow() {
  const { t } = useApp();
  const s = useSettings();
  return (
    <SwitchField
      label={t.t('ng-ui-set-pdfa')}
      hint={t.t('ng-ui-set-pdfa-hint')}
      isSelected={s.get<boolean>('signature.pdfa_force')}
      onChange={(v) => s.apply({ 'signature.pdfa_force': v })}
    />
  );
}

/**
 * „Automatische Positionierung“ wie in PDF-Over (`signature.auto_position`, `SIGNATURE_POSITION=auto`; ADR 0004
 * Nachtrag 8): sofort wirksam. Was das im Arbeitsbereich bedeutet (automatisch platziert und gesperrt bis
 * „Diesmal selbst platzieren“), entscheidet der Kern (`DocumentView::placement_locked`).
 */
export function AutoPositionRow() {
  const { t } = useApp();
  const s = useSettings();
  return (
    <SwitchField
      label={t.t('ng-ui-set-auto-position')}
      hint={t.t('ng-ui-set-auto-position-hint')}
      isSelected={s.get<boolean>('signature.auto_position')}
      onChange={(v) => s.apply({ 'signature.auto_position': v })}
    />
  );
}

/**
 * Platzhaltersuche wie in PDF-Over: ein Schalter (`placeholder.search`), darunter QR-Code (`USE_MARKER`) oder
 * leere Signaturfelder (`USE_FIELDS`); die Auswahl ist bei ausgeschalteter Suche gesperrt.
 */
export function PlaceholderRow() {
  const { t } = useApp();
  const s = useSettings();
  const search = s.get<boolean>('placeholder.search');
  return (
    <div className="stack-3">
      <SwitchField
        label={t.t('ng-ui-set-placeholder')}
        hint={t.t('ng-ui-set-placeholder-hint')}
        isSelected={search}
        onChange={(v) => s.apply({ 'placeholder.search': v })}
      />
      <ChoiceCards<PlaceholderKind>
        label={t.t('ng-ui-set-placeholder-kind')}
        value={s.get<PlaceholderKind>('placeholder.kind')}
        onChange={(v) => s.apply({ 'placeholder.kind': v })}
        isDisabled={!search}
        columns={2}
        options={[
          { value: 'marker', label: t.t('ng-ui-set-placeholder-qr'), description: t.t('ng-ui-set-placeholder-qr-desc'), icon: <IconQr size={18} /> },
          { value: 'fields', label: t.t('ng-ui-set-placeholder-field'), description: t.t('ng-ui-set-placeholder-field-desc'), icon: <IconPen size={18} /> },
        ]}
      />
    </div>
  );
}
