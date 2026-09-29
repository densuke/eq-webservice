// BGM の曲順 (ファイル名順。最後まで行ったら最初に戻る)。

/** after の次に流すファイル。after が一覧から消えていても、名前順でその次から続ける */
export function nextTrack(files: string[], after: string | null): string | null {
  const sorted = [...files].sort();
  if (!sorted.length) return null;
  if (after == null) return sorted[0];
  return sorted.find((f) => f > after) ?? sorted[0];
}
