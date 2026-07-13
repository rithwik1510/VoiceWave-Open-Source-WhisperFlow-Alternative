import type { DictionaryTerm } from "../types/voicewave";
import {
  deleteLegacyCloudDictionaryRecords,
  listCloudDictionaryRecords,
  upsertCloudDictionaryRecords
} from "./cloudSync";
import { reconcileDictionaryRecords } from "./tauri";

/**
 * Reconciles cloud data through the Rust dictionary manager, then mirrors the
 * complete winning record set back to deterministic Firestore documents.
 * Merge policy intentionally lives in Rust; this module only owns ordering.
 */
export async function syncDictionaryWithCloud(uid: string): Promise<DictionaryTerm[]> {
  const snapshot = await listCloudDictionaryRecords(uid);
  const reconciled = await reconcileDictionaryRecords(snapshot.records);
  // Rewrite only documents whose content changed or that are not yet stored
  // under their deterministic ID; rewriting the full set on every sync would
  // cost N document writes per mutation. This is a transport optimization —
  // which record wins was already decided by Rust above.
  const remoteByIdentity = new Map(
    snapshot.records.map((record) => [record.normalizedTerm, record])
  );
  const deterministic = new Set(snapshot.deterministicIdentities);
  const changedRecords = reconciled.records.filter((record) => {
    if (!deterministic.has(record.normalizedTerm)) {
      return true;
    }
    const remote = remoteByIdentity.get(record.normalizedTerm);
    return (
      !remote ||
      remote.term !== record.term ||
      remote.source !== record.source ||
      remote.createdAtUtcMs !== record.createdAtUtcMs ||
      remote.updatedAtUtcMs !== record.updatedAtUtcMs ||
      remote.deletedAtUtcMs !== record.deletedAtUtcMs
    );
  });
  await upsertCloudDictionaryRecords(uid, changedRecords);
  await deleteLegacyCloudDictionaryRecords(uid, snapshot.legacyIds);
  return reconciled.terms;
}
