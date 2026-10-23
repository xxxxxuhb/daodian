const EDITABLE_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

export function isEditableTarget(target: EventTarget | null): boolean {
  if (target == null || typeof target !== "object") return false;
  const candidate = target as { tagName?: unknown; isContentEditable?: unknown };
  return (
    (typeof candidate.tagName === "string" && EDITABLE_TAGS.has(candidate.tagName.toUpperCase())) ||
    candidate.isContentEditable === true
  );
}

export function isContainerShortcut(
  event: KeyboardEvent,
  key: string,
  blocked = false,
): boolean {
  return (
    !blocked &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === key.toLowerCase() &&
    event.target === event.currentTarget &&
    !isEditableTarget(event.target)
  );
}
