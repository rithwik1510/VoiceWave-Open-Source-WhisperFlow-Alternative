import type {
  VoiceSnippetReconcileResult,
  VoiceSnippetSyncRecord
} from "../types/voicewave";
import {
  listCloudVoiceSnippetRecords,
  upsertCloudVoiceSnippetRecords
} from "./cloudSync";
import { reconcileVoiceSnippetRecords } from "./tauri";

function recordsEqual(left: VoiceSnippetSyncRecord, right: VoiceSnippetSyncRecord): boolean {
  return (
    left.trigger === right.trigger &&
    left.normalizedTrigger === right.normalizedTrigger &&
    left.expansion === right.expansion &&
    left.createdAtUtcMs === right.createdAtUtcMs &&
    left.updatedAtUtcMs === right.updatedAtUtcMs &&
    left.deletedAtUtcMs === right.deletedAtUtcMs
  );
}

/** Reconciles cloud records through Rust, then writes only changed winners. */
export async function syncVoiceSnippetsWithCloud(
  uid: string,
  isCurrent: () => boolean = () => true
): Promise<VoiceSnippetReconcileResult> {
  if (!isCurrent()) throw new Error("Snippet sync session is no longer current.");
  const snapshot = await listCloudVoiceSnippetRecords(uid);
  if (!isCurrent()) throw new Error("Snippet sync session is no longer current.");
  const reconciled: VoiceSnippetReconcileResult = await reconcileVoiceSnippetRecords(
    snapshot.records
  );
  const remoteByIdentity = new Map(
    snapshot.records.map((record) => [record.normalizedTrigger, record])
  );
  const deterministic = new Set(snapshot.deterministicIdentities);
  const changedRecords = reconciled.records.filter((record) => {
    if (!deterministic.has(record.normalizedTrigger)) {
      return true;
    }
    const remote = remoteByIdentity.get(record.normalizedTrigger);
    return !remote || !recordsEqual(remote, record);
  });
  if (!isCurrent()) throw new Error("Snippet sync session is no longer current.");
  await upsertCloudVoiceSnippetRecords(uid, changedRecords);
  return reconciled;
}
