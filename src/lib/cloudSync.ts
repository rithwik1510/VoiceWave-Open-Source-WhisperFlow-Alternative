import type { Auth, User } from "firebase/auth";
import type { Firestore } from "firebase/firestore";
import { firebaseEnabled, getFirebase } from "./firebase";
import type { DictionarySyncRecord, VoiceSnippetSyncRecord } from "../types/voicewave";

// The Firebase SDK is loaded lazily so it stays out of the main bundle. These
// thin loaders wrap the dynamic imports; `vi.mock` intercepts them in tests.
function loadAuthSdk() {
  return import("firebase/auth");
}

function loadFirestoreSdk() {
  return import("firebase/firestore");
}

/**
 * Resolves the initialized Firebase handles, throwing the same "not configured"
 * error the previous synchronous `requireCloud` guard produced.
 */
async function getCloud(): Promise<{ auth: Auth; db: Firestore }> {
  const handles = firebaseEnabled ? await getFirebase() : null;
  if (!handles) {
    throw new Error(
      "Firebase is not configured. Set VITE_FIREBASE_* variables to enable cloud auth and sync."
    );
  }
  return handles;
}

const MAX_RECENT_SENTENCES = 5;
const MAX_NAME_LENGTH = 80;
const MAX_EMAIL_LENGTH = 200;
const MAX_WORKSPACE_ROLE_LENGTH = 80;
const MAX_SENTENCE_LENGTH = 512;
const MAX_SOURCE_LENGTH = 40;
const MIN_SENTENCE_WRITE_INTERVAL_MS = 700;
const MIN_DICTIONARY_WRITE_INTERVAL_MS = 500;
const MIN_SNIPPET_WRITE_INTERVAL_MS = 500;
const MAX_WRITE_BACKOFF_MS = 8_000;
const MAX_WRITE_ATTEMPTS = 3;
const MIN_SYNC_TIMESTAMP_MS = 1_609_459_200_000;
const MAX_FUTURE_TIMESTAMP_SKEW_MS = 5 * 60 * 1_000;

type CloudGuardrailSeverity = "info" | "warn" | "error";

interface WriteGuardrailState {
  lastWriteMs: number;
  lastContentHash: string | null;
  consecutiveFailures: number;
  nextAllowedWriteMs: number;
}

export interface CloudSyncErrorShape {
  code: string;
  retryable: boolean;
  context: string;
  message: string;
}

export class CloudSyncError extends Error implements CloudSyncErrorShape {
  code: string;
  retryable: boolean;
  context: string;

  constructor(payload: CloudSyncErrorShape) {
    super(payload.message);
    this.name = "CloudSyncError";
    this.code = payload.code;
    this.retryable = payload.retryable;
    this.context = payload.context;
  }
}

const sentenceWriteState = new Map<string, WriteGuardrailState>();
const dictionaryWriteState = new Map<string, WriteGuardrailState>();
const snippetWriteState = new Map<string, WriteGuardrailState>();

export interface CloudProfile {
  uid: string;
  name: string;
  email: string;
  workspaceRole: string;
}

export interface CloudSentence {
  id: string;
  text: string;
  createdAtUtcMs: number;
}

export interface CloudDictionarySnapshot {
  records: DictionarySyncRecord[];
  legacyIds: string[];
  /** Identities already stored under their deterministic document ID with the
   * current schema — safe to skip rewriting when their content is unchanged. */
  deterministicIdentities: string[];
}

export interface CloudVoiceSnippetSnapshot {
  records: VoiceSnippetSyncRecord[];
  /** Identities already stored under their deterministic document ID and safe
   * to omit from a changed-only write. */
  deterministicIdentities: string[];
}

export function normalizeDictionaryIdentity(value: string): string {
  return value.trim().replace(/\s+/gu, " ").normalize("NFC").toLowerCase();
}

export function dictionaryDocumentId(normalizedTerm: string): string {
  return `term-${encodeURIComponent(normalizedTerm)}`;
}

export function normalizeSnippetIdentity(value: string): string {
  return value.trim().replace(/\s+/gu, " ").normalize("NFC").toLowerCase();
}

export function snippetDocumentId(normalizedTrigger: string): string {
  return `snippet-${encodeURIComponent(normalizedTrigger)}`;
}

function mapDictionaryRecordRow(row: {
  id: string;
  data: () => Record<string, unknown>;
}): { record: DictionarySyncRecord; legacy: boolean } {
  const data = row.data();
  const term = typeof data.term === "string" ? data.term : "";
  const schemaLegacy =
    !("normalizedTerm" in data) ||
    !("updatedAtUtcMs" in data) ||
    !("deletedAtUtcMs" in data);
  // Legacy rows normalized with trim+lowercase only. Recompute their identity
  // under the current contract so Rust's fail-closed identity check cannot
  // reject the entire sync over a stale stored `termNormalized` value.
  const normalizedTerm =
    !schemaLegacy && typeof data.normalizedTerm === "string"
      ? data.normalizedTerm
      : normalizeDictionaryIdentity(term);
  const createdAtUtcMs =
    typeof data.createdAtUtcMs === "number" ? data.createdAtUtcMs : Date.now();
  return {
    record: {
      term,
      normalizedTerm,
      source: typeof data.source === "string" ? data.source : "cloud-sync",
      createdAtUtcMs,
      updatedAtUtcMs:
        typeof data.updatedAtUtcMs === "number" ? data.updatedAtUtcMs : createdAtUtcMs,
      deletedAtUtcMs: typeof data.deletedAtUtcMs === "number" ? data.deletedAtUtcMs : null
    },
    legacy: schemaLegacy || row.id !== dictionaryDocumentId(normalizedTerm)
  };
}

// Mirrors the Rust validation contract (`normalize_and_validate_term`) closely
// enough to keep one malformed cloud document from aborting reconciliation.
function isSyncableDictionaryTerm(term: string): boolean {
  if (/\p{Cc}/u.test(term)) {
    return false;
  }
  const display = term.trim().replace(/\s+/gu, " ").normalize("NFC");
  return display.length > 0 && [...display].length <= 72;
}

const FORBIDDEN_EXPANSION_CHARACTERS = /[\u0000-\u0008\u000B\u000C\u000D-\u001F\u007F-\u009F\uE000\uE001]/u;
const RESERVED_SNIPPET_TRIGGERS = new Set([
  "new line",
  "next line",
  "new paragraph",
  "bullet point",
  "new bullet"
]);

function isSyncableSnippetTrigger(trigger: string): boolean {
  if (/\p{Cc}/u.test(trigger)) {
    return false;
  }
  const display = trigger.trim().normalize("NFC");
  const identity = normalizeSnippetIdentity(display);
  return display.length > 0 && [...display].length <= 60 && !RESERVED_SNIPPET_TRIGGERS.has(identity);
}

function isSyncableSnippetExpansion(expansion: string): boolean {
  return (
    expansion.trim().length > 0 &&
    [...expansion].length <= 4_000 &&
    !FORBIDDEN_EXPANSION_CHARACTERS.test(expansion)
  );
}

function isSyncableTimestamp(value: unknown, nowUtcMs: number): value is number {
  return (
    typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= MIN_SYNC_TIMESTAMP_MS &&
    value <= nowUtcMs + MAX_FUTURE_TIMESTAMP_SKEW_MS
  );
}

function mapVoiceSnippetRecordRow(row: {
  id: string;
  data: () => Record<string, unknown>;
}): { record: VoiceSnippetSyncRecord; deterministic: boolean } | null {
  const data = row.data();
  const trigger = typeof data.trigger === "string" ? data.trigger : "";
  const normalizedTrigger =
    typeof data.normalizedTrigger === "string" ? data.normalizedTrigger : "";
  const expansion = typeof data.expansion === "string" ? data.expansion : "";
  const createdAtUtcMs = data.createdAtUtcMs;
  const updatedAtUtcMs = data.updatedAtUtcMs;
  const deletedAtUtcMs = data.deletedAtUtcMs;
  const nowUtcMs = Date.now();

  if (
    !isSyncableSnippetTrigger(trigger) ||
    // A row whose stored identity disagrees with its trigger would be
    // rejected fail-closed by Rust reconciliation, permanently aborting every
    // sync. Quarantine it here instead; reads never delete or rewrite it.
    normalizedTrigger !== normalizeSnippetIdentity(trigger) ||
    !isSyncableTimestamp(createdAtUtcMs, nowUtcMs) ||
    !isSyncableTimestamp(updatedAtUtcMs, nowUtcMs) ||
    updatedAtUtcMs < createdAtUtcMs ||
    (deletedAtUtcMs !== null &&
      (!isSyncableTimestamp(deletedAtUtcMs, nowUtcMs) ||
        deletedAtUtcMs < updatedAtUtcMs)) ||
    (deletedAtUtcMs === null
      ? !isSyncableSnippetExpansion(expansion)
      : expansion !== "")
  ) {
    return null;
  }

  const record = {
    trigger,
    normalizedTrigger,
    expansion,
    createdAtUtcMs,
    updatedAtUtcMs,
    deletedAtUtcMs
  };
  return {
    record,
    deterministic: row.id === snippetDocumentId(normalizedTrigger)
  };
}

function clampTrimmed(value: string, max: number): string {
  const trimmed = value.trim();
  if (trimmed.length <= max) {
    return trimmed;
  }
  return trimmed.slice(0, max);
}

function hashPayload(value: string): string {
  return value.toLowerCase();
}

function hashPrivatePayload(value: string): string {
  // FNV-1a is enough for local duplicate-write suppression. It prevents
  // private snippet identity from being retained in guardrail state.
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return `snippet-${(hash >>> 0).toString(16)}-${value.length}`;
}

function getWriteState(
  registry: Map<string, WriteGuardrailState>,
  key: string
): WriteGuardrailState {
  const existing = registry.get(key);
  if (existing) {
    return existing;
  }
  const initial: WriteGuardrailState = {
    lastWriteMs: 0,
    lastContentHash: null,
    consecutiveFailures: 0,
    nextAllowedWriteMs: 0
  };
  registry.set(key, initial);
  return initial;
}

function emitCloudGuardrailEvent(
  event: string,
  context: string,
  severity: CloudGuardrailSeverity,
  detail: string
): void {
  const payload = {
    event,
    context,
    severity,
    detail,
    atUtcMs: Date.now()
  };
  if (severity === "error") {
    console.error("[CloudGuardrail]", payload);
  } else if (severity === "warn") {
    console.warn("[CloudGuardrail]", payload);
  } else {
    console.info("[CloudGuardrail]", payload);
  }
}

function readFirebaseCode(error: unknown): string {
  if (!error || typeof error !== "object" || !("code" in error)) {
    return "unknown";
  }
  return String((error as { code: unknown }).code);
}

function isRetryableFirebaseCode(code: string): boolean {
  return (
    code === "unavailable" ||
    code === "aborted" ||
    code === "deadline-exceeded" ||
    code === "resource-exhausted" ||
    code === "internal" ||
    code === "unknown" ||
    code === "auth/network-request-failed"
  );
}

function normalizeCloudError(error: unknown, context: string): CloudSyncError {
  const code = readFirebaseCode(error);
  const message = readFirebaseMessage(error);
  const retryable = isRetryableFirebaseCode(code);
  if (code === "permission-denied") {
    emitCloudGuardrailEvent("cloud_rule_rejection", context, "warn", message);
  }
  return new CloudSyncError({
    code,
    retryable,
    context,
    message
  });
}

function registerWriteSuccess(state: WriteGuardrailState, writeMs: number, contentHash: string): void {
  state.lastWriteMs = writeMs;
  state.lastContentHash = contentHash;
  state.consecutiveFailures = 0;
  state.nextAllowedWriteMs = 0;
}

function registerWriteFailure(state: WriteGuardrailState, nowUtcMs: number): number {
  state.consecutiveFailures += 1;
  const delay = Math.min(MAX_WRITE_BACKOFF_MS, Math.pow(2, state.consecutiveFailures - 1) * 500);
  state.nextAllowedWriteMs = nowUtcMs + delay;
  return delay;
}

function enforceClientBackpressure(
  state: WriteGuardrailState,
  nowUtcMs: number,
  contentHash: string,
  minIntervalMs: number,
  context: string
): void {
  if (state.nextAllowedWriteMs > nowUtcMs) {
    const waitMs = state.nextAllowedWriteMs - nowUtcMs;
    throw new CloudSyncError({
      code: "client-backpressure",
      retryable: true,
      context,
      message: `Cloud sync temporarily throttled. Retry in ${waitMs}ms.`
    });
  }

  if (
    state.lastContentHash === contentHash &&
    nowUtcMs - state.lastWriteMs < minIntervalMs
  ) {
    throw new CloudSyncError({
      code: "client-dedup",
      retryable: false,
      context,
      message: "Skipped duplicate cloud write."
    });
  }
}

async function withCloudRetry<T>(
  operation: () => Promise<T>,
  context: string,
  state: WriteGuardrailState,
  contentHash: string
): Promise<T> {
  let lastError: CloudSyncError | null = null;
  for (let attempt = 1; attempt <= MAX_WRITE_ATTEMPTS; attempt += 1) {
    try {
      const result = await operation();
      registerWriteSuccess(state, Date.now(), contentHash);
      return result;
    } catch (error) {
      const normalized = normalizeCloudError(error, context);
      lastError = normalized;
      if (!normalized.retryable || attempt >= MAX_WRITE_ATTEMPTS) {
        registerWriteFailure(state, Date.now());
        throw normalized;
      }
      const backoffMs = registerWriteFailure(state, Date.now());
      await new Promise((resolve) => window.setTimeout(resolve, backoffMs));
    }
  }
  throw (
    lastError ??
    new CloudSyncError({
      code: "unknown",
      retryable: false,
      context,
      message: "Cloud request failed."
    })
  );
}

function readFirebaseMessage(error: unknown): string {
  if (!error || typeof error !== "object") {
    return "Cloud request failed.";
  }
  const code = "code" in error ? String((error as { code: unknown }).code) : "";
  if (code === "auth/invalid-credential" || code === "auth/wrong-password" || code === "auth/user-not-found") {
    return "Invalid email or password.";
  }
  if (code === "auth/email-already-in-use") {
    return "This email already has an account.";
  }
  if (code === "auth/weak-password") {
    return "Password is too weak. Use at least 6 characters.";
  }
  if (code === "auth/too-many-requests") {
    return "Too many attempts. Please try again shortly.";
  }
  if (code === "auth/invalid-email") {
    return "Please enter a valid email address.";
  }
  if (code === "auth/missing-email") {
    return "Email is required for this action.";
  }
  if (code === "auth/popup-closed-by-user") {
    return "Google sign-in popup was closed before completing.";
  }
  if (code === "auth/popup-blocked") {
    return "Popup was blocked. Please allow popups and try again.";
  }
  if (code === "auth/cancelled-popup-request") {
    return "Another sign-in request is already in progress.";
  }
  if (code === "permission-denied") {
    return "Cloud write blocked by server policy. Check account and payload constraints.";
  }
  if (code === "resource-exhausted") {
    return "Cloud service is rate-limiting requests. Please retry shortly.";
  }
  if ("message" in error && typeof (error as { message: unknown }).message === "string") {
    return (error as { message: string }).message;
  }
  return "Cloud request failed.";
}

function toProfile(user: User, workspaceRole = "Personal Workspace"): CloudProfile {
  const fallbackName = user.email?.split("@")[0] ?? "VoiceWave User";
  return {
    uid: user.uid,
    name: user.displayName ?? fallbackName,
    email: user.email ?? "",
    workspaceRole
  };
}

export function getCloudErrorMessage(error: unknown): string {
  return readFirebaseMessage(error);
}

export function subscribeCloudAuth(listener: (user: User | null) => void): () => void {
  if (!firebaseEnabled) {
    throw new Error(
      "Firebase is not configured. Set VITE_FIREBASE_* variables to enable cloud auth and sync."
    );
  }
  let unsubscribe: (() => void) | null = null;
  let cancelled = false;
  void (async () => {
    const { auth } = await getCloud();
    const { onAuthStateChanged } = await loadAuthSdk();
    if (cancelled) {
      return;
    }
    unsubscribe = onAuthStateChanged(auth, listener);
  })();
  return () => {
    cancelled = true;
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
  };
}

export async function signUpCloud(input: {
  email: string;
  password: string;
  name: string;
  workspaceRole: string;
}): Promise<CloudProfile> {
  const { auth, db } = await getCloud();
  const { createUserWithEmailAndPassword, updateProfile } = await loadAuthSdk();
  const { doc, setDoc } = await loadFirestoreSdk();
  try {
    const email = clampTrimmed(input.email, MAX_EMAIL_LENGTH);
    const nameInput = clampTrimmed(input.name, MAX_NAME_LENGTH);
    const workspaceRole = clampTrimmed(input.workspaceRole, MAX_WORKSPACE_ROLE_LENGTH);
    const credential = await createUserWithEmailAndPassword(
      auth,
      email,
      input.password
    );
    const name = nameInput || credential.user.email?.split("@")[0] || "VoiceWave User";
    await updateProfile(credential.user, { displayName: name });

    const profile = toProfile(credential.user, workspaceRole || "Personal Workspace");
    await setDoc(
      doc(db, "users", credential.user.uid),
      {
        name: profile.name,
        email: profile.email,
        workspaceRole: profile.workspaceRole,
        createdAtUtcMs: Date.now(),
        updatedAtUtcMs: Date.now()
      },
      { merge: true }
    );
    return profile;
  } catch (error) {
    throw normalizeCloudError(error, "signup");
  }
}

export async function signInCloud(email: string, password: string): Promise<CloudProfile> {
  const { auth } = await getCloud();
  const { signInWithEmailAndPassword } = await loadAuthSdk();
  try {
    const credential = await signInWithEmailAndPassword(
      auth,
      clampTrimmed(email, MAX_EMAIL_LENGTH),
      password
    );
    return ensureCloudProfile(credential.user, "Personal Workspace");
  } catch (error) {
    throw normalizeCloudError(error, "signin");
  }
}

export async function signInWithGoogleCloud(): Promise<CloudProfile> {
  const { auth } = await getCloud();
  const { GoogleAuthProvider, signInWithPopup, signInWithRedirect } = await loadAuthSdk();
  const provider = new GoogleAuthProvider();
  provider.setCustomParameters({ prompt: "select_account" });

  try {
    const credential = await signInWithPopup(auth, provider);
    return ensureCloudProfile(credential.user, "Personal Workspace");
  } catch (error) {
    const code = error && typeof error === "object" && "code" in error
      ? String((error as { code: unknown }).code)
      : "";

    if (code === "auth/popup-blocked") {
      await signInWithRedirect(auth, provider);
      throw new Error("GOOGLE_REDIRECT_STARTED");
    }

    throw normalizeCloudError(error, "google-signin");
  }
}

export async function completeGoogleRedirectSignIn(): Promise<CloudProfile | null> {
  const { auth } = await getCloud();
  const { getRedirectResult } = await loadAuthSdk();
  try {
    const credential = await getRedirectResult(auth);
    if (!credential?.user) {
      return null;
    }
    return ensureCloudProfile(credential.user, "Personal Workspace");
  } catch (error) {
    throw normalizeCloudError(error, "google-redirect");
  }
}

export async function signOutCloud(): Promise<void> {
  const { auth } = await getCloud();
  const { signOut } = await loadAuthSdk();
  try {
    await signOut(auth);
  } catch (error) {
    throw normalizeCloudError(error, "signout");
  }
}

export async function requestPasswordResetCloud(email: string): Promise<void> {
  const { auth } = await getCloud();
  const { sendPasswordResetEmail } = await loadAuthSdk();
  try {
    await sendPasswordResetEmail(auth, clampTrimmed(email, MAX_EMAIL_LENGTH));
  } catch (error) {
    throw normalizeCloudError(error, "password-reset");
  }
}

export async function ensureCloudProfile(user: User, fallbackWorkspaceRole: string): Promise<CloudProfile> {
  const { db } = await getCloud();
  const { doc, getDoc, setDoc } = await loadFirestoreSdk();
  try {
    const userRef = doc(db, "users", user.uid);
    const existing = await getDoc(userRef);
    const baseProfile = toProfile(user, clampTrimmed(fallbackWorkspaceRole, MAX_WORKSPACE_ROLE_LENGTH));

    if (existing.exists()) {
      const data = existing.data();
      return {
        uid: user.uid,
        name:
          typeof data.name === "string" && data.name.trim().length > 0
            ? clampTrimmed(data.name, MAX_NAME_LENGTH)
            : baseProfile.name,
        email:
          typeof data.email === "string" && data.email.trim().length > 0
            ? clampTrimmed(data.email, MAX_EMAIL_LENGTH)
            : baseProfile.email,
        workspaceRole:
          typeof data.workspaceRole === "string" && data.workspaceRole.trim().length > 0
            ? clampTrimmed(data.workspaceRole, MAX_WORKSPACE_ROLE_LENGTH)
            : baseProfile.workspaceRole
      };
    }

    await setDoc(userRef, {
      name: baseProfile.name,
      email: baseProfile.email,
      workspaceRole: baseProfile.workspaceRole,
      createdAtUtcMs: Date.now(),
      updatedAtUtcMs: Date.now()
    });
    return baseProfile;
  } catch (error) {
    throw normalizeCloudError(error, "ensure-profile");
  }
}

export async function listRecentCloudSentences(uid: string): Promise<CloudSentence[]> {
  const { db } = await getCloud();
  const { collection, getDocs, limit, orderBy, query } = await loadFirestoreSdk();
  const rows = await getDocs(
    query(
      collection(db, "users", uid, "recentSentences"),
      orderBy("createdAtUtcMs", "desc"),
      limit(MAX_RECENT_SENTENCES)
    )
  );

  return rows.docs.map((row) => {
    const data = row.data();
    return {
      id: row.id,
      text: typeof data.text === "string" ? data.text : "",
      createdAtUtcMs: typeof data.createdAtUtcMs === "number" ? data.createdAtUtcMs : Date.now()
    };
  });
}

export async function saveCloudSentence(uid: string, text: string): Promise<CloudSentence[]> {
  const { db } = await getCloud();
  const { collection, deleteDoc, doc, getDocs, orderBy, query, setDoc } =
    await loadFirestoreSdk();
  const normalized = clampTrimmed(text, MAX_SENTENCE_LENGTH);
  if (!normalized) {
    return listRecentCloudSentences(uid);
  }

  const state = getWriteState(sentenceWriteState, uid);
  const contentHash = hashPayload(normalized);
  try {
    enforceClientBackpressure(state, Date.now(), contentHash, MIN_SENTENCE_WRITE_INTERVAL_MS, "save-sentence");
  } catch (error) {
    if (error instanceof CloudSyncError && (error.code === "client-dedup" || error.code === "client-backpressure")) {
      emitCloudGuardrailEvent("cloud_write_skipped", "save-sentence", "info", error.message);
      return listRecentCloudSentences(uid);
    }
    throw error;
  }

  const context = "save-sentence";
  return withCloudRetry(async () => {
    const rowRef = doc(collection(db, "users", uid, "recentSentences"));
    await setDoc(rowRef, {
      text: normalized,
      createdAtUtcMs: Date.now()
    });

    const recentRows = await getDocs(
      query(collection(db, "users", uid, "recentSentences"), orderBy("createdAtUtcMs", "desc"))
    );
    const stale = recentRows.docs.slice(MAX_RECENT_SENTENCES);
    await Promise.all(stale.map((entry) => deleteDoc(entry.ref)));

    return recentRows.docs.slice(0, MAX_RECENT_SENTENCES).map((row) => {
      const data = row.data();
      return {
        id: row.id,
        text: typeof data.text === "string" ? data.text : "",
        createdAtUtcMs: typeof data.createdAtUtcMs === "number" ? data.createdAtUtcMs : Date.now()
      };
    });
  }, context, state, contentHash);
}

export async function listCloudDictionaryRecords(uid: string): Promise<CloudDictionarySnapshot> {
  const { db } = await getCloud();
  const { collection, getDocs } = await loadFirestoreSdk();
  const rows = await getDocs(collection(db, "users", uid, "dictionaryTerms"));
  const mapped = rows.docs
    .map((row) => ({
      id: row.id,
      ...mapDictionaryRecordRow({ id: row.id, data: () => row.data() })
    }))
    // A row the local contract can never accept (empty, control characters,
    // overlength) is skipped — never sent to Rust, never deleted — instead of
    // poisoning every future sync with a validation error.
    .filter(({ record }) => isSyncableDictionaryTerm(record.term));
  return {
    records: mapped.map(({ record }) => record),
    legacyIds: mapped
      .filter(({ id, legacy, record }) =>
        legacy && id !== dictionaryDocumentId(record.normalizedTerm)
      )
      .map(({ id }) => id),
    deterministicIdentities: mapped
      .filter(({ legacy }) => !legacy)
      .map(({ record }) => record.normalizedTerm)
  };
}

export async function upsertCloudDictionaryRecords(
  uid: string,
  records: DictionarySyncRecord[]
): Promise<void> {
  const { db } = await getCloud();
  const { doc, writeBatch } = await loadFirestoreSdk();
  const validated = records.map((record) => {
    const normalizedTerm = normalizeDictionaryIdentity(record.term);
    if (normalizedTerm !== record.normalizedTerm) {
      throw new CloudSyncError({
        code: "dictionary-identity-mismatch",
        retryable: false,
        context: "sync-dictionary",
        message: "Dictionary identity mismatch between local storage and cloud sync."
      });
    }
    return record;
  });
  if (validated.length === 0) {
    return;
  }
  const state = getWriteState(dictionaryWriteState, uid);
  const contentHash = hashPayload(
    validated
      .map((record) => `${record.normalizedTerm}:${record.updatedAtUtcMs}:${record.deletedAtUtcMs ?? "active"}`)
      .sort()
      .join("|")
  );
  try {
    enforceClientBackpressure(
      state,
      Date.now(),
      contentHash,
      MIN_DICTIONARY_WRITE_INTERVAL_MS,
      "sync-dictionary"
    );
  } catch (error) {
    if (error instanceof CloudSyncError && error.code === "client-dedup") {
      emitCloudGuardrailEvent("cloud_write_skipped", "sync-dictionary", "info", error.message);
      return;
    }
    throw error;
  }

  await withCloudRetry(async () => {
    for (let start = 0; start < validated.length; start += 500) {
      const batch = writeBatch(db);
      for (const record of validated.slice(start, start + 500)) {
        batch.set(doc(db, "users", uid, "dictionaryTerms", dictionaryDocumentId(record.normalizedTerm)), {
          term: record.term,
          normalizedTerm: record.normalizedTerm,
          source: clampTrimmed(record.source, MAX_SOURCE_LENGTH) || "sync",
          createdAtUtcMs: record.createdAtUtcMs,
          updatedAtUtcMs: record.updatedAtUtcMs,
          deletedAtUtcMs: record.deletedAtUtcMs
        });
      }
      await batch.commit();
    }
  }, "sync-dictionary", state, contentHash);
}

export async function deleteLegacyCloudDictionaryRecords(
  uid: string,
  legacyIds: string[]
): Promise<void> {
  if (legacyIds.length === 0) return;
  const { db } = await getCloud();
  const { doc, writeBatch } = await loadFirestoreSdk();
  for (let start = 0; start < legacyIds.length; start += 500) {
    const batch = writeBatch(db);
    for (const legacyId of legacyIds.slice(start, start + 500)) {
      batch.delete(doc(db, "users", uid, "dictionaryTerms", legacyId));
    }
    await batch.commit();
  }
}

export async function listCloudVoiceSnippetRecords(
  uid: string
): Promise<CloudVoiceSnippetSnapshot> {
  const { db } = await getCloud();
  const { collection, getDocs } = await loadFirestoreSdk();
  const rows = await getDocs(collection(db, "users", uid, "voiceSnippets"));
  const mapped = rows.docs
    .map((row) => mapVoiceSnippetRecordRow({ id: row.id, data: () => row.data() }))
    // Quarantine malformed rows in place. Reads never delete or rewrite them.
    .filter((row): row is NonNullable<typeof row> => row !== null);
  return {
    records: mapped.map(({ record }) => record),
    deterministicIdentities: mapped
      .filter(({ deterministic }) => deterministic)
      .map(({ record }) => record.normalizedTrigger)
  };
}

export async function upsertCloudVoiceSnippetRecords(
  uid: string,
  records: VoiceSnippetSyncRecord[]
): Promise<void> {
  const validated = records.map((record) => {
    const normalizedTrigger = normalizeSnippetIdentity(record.trigger);
    if (normalizedTrigger !== record.normalizedTrigger) {
      throw new CloudSyncError({
        code: "snippet-identity-mismatch",
        retryable: false,
        context: "sync-snippets",
        message: "Snippet identity mismatch between local storage and cloud sync."
      });
    }
    if (
      !isSyncableSnippetTrigger(record.trigger) ||
      !Number.isSafeInteger(record.createdAtUtcMs) ||
      !Number.isSafeInteger(record.updatedAtUtcMs) ||
      record.updatedAtUtcMs < record.createdAtUtcMs ||
      (record.deletedAtUtcMs !== null &&
        (!Number.isSafeInteger(record.deletedAtUtcMs) ||
          record.deletedAtUtcMs < record.updatedAtUtcMs)) ||
      (record.deletedAtUtcMs === null
        ? !isSyncableSnippetExpansion(record.expansion)
        : record.expansion !== "")
    ) {
      throw new CloudSyncError({
        code: "snippet-validation",
        retryable: false,
        context: "sync-snippets",
        message: "Snippet data does not satisfy the cloud sync contract."
      });
    }
    return record;
  });
  if (validated.length === 0) {
    return;
  }

  const { db } = await getCloud();
  const { doc, writeBatch } = await loadFirestoreSdk();
  const state = getWriteState(snippetWriteState, uid);
  // Content is deliberately excluded: guardrail diagnostics and write-state
  // keys must never retain private trigger or expansion text.
  const contentHash = hashPrivatePayload(
    JSON.stringify(
      validated
        .map((record) => ({
          trigger: record.trigger,
          normalizedTrigger: record.normalizedTrigger,
          expansion: record.expansion,
          createdAtUtcMs: record.createdAtUtcMs,
          updatedAtUtcMs: record.updatedAtUtcMs,
          deletedAtUtcMs: record.deletedAtUtcMs
        }))
        .sort((left, right) => left.normalizedTrigger.localeCompare(right.normalizedTrigger))
    )
  );
  try {
    enforceClientBackpressure(
      state,
      Date.now(),
      contentHash,
      MIN_SNIPPET_WRITE_INTERVAL_MS,
      "sync-snippets"
    );
  } catch (error) {
    if (error instanceof CloudSyncError && error.code === "client-dedup") {
      emitCloudGuardrailEvent("cloud_write_skipped", "sync-snippets", "info", error.message);
      return;
    }
    throw error;
  }

  await withCloudRetry(async () => {
    for (let start = 0; start < validated.length; start += 500) {
      const batch = writeBatch(db);
      for (const record of validated.slice(start, start + 500)) {
        batch.set(
          doc(
            db,
            "users",
            uid,
            "voiceSnippets",
            snippetDocumentId(record.normalizedTrigger)
          ),
          {
            trigger: record.trigger,
            normalizedTrigger: record.normalizedTrigger,
            expansion: record.expansion,
            createdAtUtcMs: record.createdAtUtcMs,
            updatedAtUtcMs: record.updatedAtUtcMs,
            deletedAtUtcMs: record.deletedAtUtcMs
          }
        );
      }
      await batch.commit();
    }
  }, "sync-snippets", state, contentHash);
}
