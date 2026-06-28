import { Blocks, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { MinecraftBlockName } from "../domain/schematicModel";
import {
  loadMinecraftBlockCatalog,
  minecraftBlockIcon,
  minecraftBlockLabel,
  minecraftBlockTint,
  MINECRAFT_ASSET_VERSION,
  type MinecraftBlockCatalogEntry
} from "../services/minecraftBlockCatalog";

export function MinecraftBlockIcon({ block, size = 28 }: { block: string; size?: number }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <Blocks aria-hidden="true" width={size} height={size} />;
  const tint = minecraftBlockTint(block);
  const image = <img className={tint ? "minecraft-block-icon biome-tinted" : "minecraft-block-icon"} src={minecraftBlockIcon(block)} width={size} height={size} alt="" onError={() => setFailed(true)} />;
  return tint ? <span className="minecraft-block-icon-tint" style={{ width: size, height: size, backgroundColor: tint }}>{image}</span> : image;
}

export function MinecraftBlockPicker({ value, onChange, label, searchLabel, excludeAir = true, allowedBlocks }: {
  value: MinecraftBlockName | string;
  onChange: (block: MinecraftBlockName) => void;
  label: string;
  searchLabel: string;
  excludeAir?: boolean;
  allowedBlocks?: readonly string[];
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [blocks, setBlocks] = useState<MinecraftBlockCatalogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(1);

  useEffect(() => {
    void loadMinecraftBlockCatalog().then(setBlocks).catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, []);
  useEffect(() => setPage(1), [query]);

  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase().replace(/\s+/g, "_");
    return blocks.filter((block) =>
      (!allowedBlocks || allowedBlocks.includes(block.id)) &&
      (!excludeAir || !["minecraft:air", "minecraft:cave_air", "minecraft:void_air"].includes(block.id)) &&
      (!normalized || block.id.includes(normalized) || block.label.includes(query.trim().toLowerCase()))
    );
  }, [allowedBlocks, blocks, excludeAir, query]);
  const visible = filtered.slice(0, page * 120);

  return <div className="minecraft-block-picker">
    <span className="block-picker-label">{label}</span>
    <button type="button" className="block-picker-trigger" onClick={() => setOpen((value) => !value)} aria-expanded={open}>
      <MinecraftBlockIcon block={value} />
      <span>{minecraftBlockLabel(value)}</span>
      <small>Java {MINECRAFT_ASSET_VERSION}</small>
    </button>
    {open ? <div className="block-picker-popover">
      <div className="block-picker-search">
        <Search aria-hidden="true" />
        <input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder={searchLabel} />
        <button type="button" className="icon-button" onClick={() => setOpen(false)} aria-label="Close"><X aria-hidden="true" /></button>
      </div>
      <p className="block-picker-count">{filtered.length} / {blocks.length}</p>
      {error ? <p className="schematic-error">{error}</p> : null}
      <div className="block-picker-grid">
        {visible.map((block) => <button type="button" className={block.id === value ? "block-option selected" : "block-option"} key={block.id} onClick={() => { onChange(block.id); setOpen(false); }} title={block.id}>
          {block.icon ? <MinecraftBlockIcon block={block.id} size={32} /> : <Blocks aria-hidden="true" />}
          <span>{block.label}</span>
        </button>)}
      </div>
      {visible.length < filtered.length ? <button type="button" className="secondary-action compact-action" onClick={() => setPage((value) => value + 1)}>+ {Math.min(120, filtered.length - visible.length)}</button> : null}
    </div> : null}
  </div>;
}
