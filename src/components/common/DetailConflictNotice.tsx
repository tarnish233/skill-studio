/** Keep the editor mounted; a refresh must not discard its draft or navigation guard. */
export function DetailConflictNotice({
  missing,
  changed = false,
}: {
  missing: boolean;
  changed?: boolean;
}) {
  if (!missing && !changed) return null;
  return (
    <p role="alert" className="text-sm text-amber-600 dark:text-amber-400">
      {missing
        ? "记录已在别处删除，当前草稿仍保留；不能保存或部署，离开前可复制需要的内容。"
        : "记录已在别处修改，当前草稿仍保留；保存时会检查原版本，不会覆盖别处的修改。"}
    </p>
  );
}
