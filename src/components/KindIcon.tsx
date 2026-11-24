// Icons for what a download holds. A whole download gets a general type glyph (film,
// gamepad, music…); a single file in its file list gets the Windows shell icon for its
// extension (what Explorer shows), with the glyph as fallback while it loads or when
// Windows has none.
import { useEffect, useState } from "react";
import {
  Archive,
  AppWindow,
  BookOpen,
  File,
  Film,
  Gamepad2,
  Image,
  Music,
  type LucideIcon,
} from "lucide-react";
import { api } from "../lib/api";
import type { ContentKind } from "../lib/types";

const GLYPH: Record<ContentKind, [LucideIcon, string, string]> = {
  video: [Film, "text-blue", "Video"],
  audio: [Music, "text-pink", "Audio"],
  game: [Gamepad2, "text-violet", "Game"],
  software: [AppWindow, "text-good", "Software"],
  book: [BookOpen, "text-warn", "Book"],
  image: [Image, "text-pink", "Image"],
  archive: [Archive, "text-dim", "Archive"],
  other: [File, "text-faint", "File"],
};

const icons = new Map<string, Promise<string | null>>();
function shellIcon(ext: string): Promise<string | null> {
  let p = icons.get(ext);
  if (!p) {
    p = api.fileIcon(ext).then(
      (buf) => URL.createObjectURL(new Blob([buf], { type: "image/png" })),
      () => null,
    );
    icons.set(ext, p);
  }
  return p;
}

/** General glyph for a download's content kind. */
export function KindIcon({
  kind,
  ext,
  size = 16,
}: {
  kind: ContentKind;
  ext?: string | null;
  size?: number;
}) {
  const [Glyph, tone, label] = GLYPH[kind] ?? GLYPH.other;
  return (
    <span
      title={ext ? `${label} (.${ext})` : label}
      className="flex shrink-0 items-center justify-center"
      style={{ width: size, height: size }}
    >
      <Glyph size={size - 2} className={tone} />
    </span>
  );
}

/** Windows shell icon for a single file, by extension. */
export function FileIcon({ path, size = 16 }: { path: string; size?: number }) {
  const ext = extOf(path);
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    setSrc(null);
    if (!ext) return;
    let live = true;
    shellIcon(ext).then((u) => live && setSrc(u));
    return () => {
      live = false;
    };
  }, [ext]);
  const title = ext ? `.${ext} file` : "File";
  return src ? (
    <img
      src={src}
      width={size}
      height={size}
      alt=""
      title={title}
      draggable={false}
      className="shrink-0"
    />
  ) : (
    <span
      title={title}
      className="flex shrink-0 items-center justify-center"
      style={{ width: size, height: size }}
    >
      <File size={size - 2} className="text-faint" />
    </span>
  );
}

/** Lowercase extension of a path, if it has a plausible one. */
export function extOf(path: string): string | null {
  const m = /\.([a-z0-9]{1,9})$/i.exec(path.split(/[\\/]/).pop() ?? "");
  return m ? m[1].toLowerCase() : null;
}
