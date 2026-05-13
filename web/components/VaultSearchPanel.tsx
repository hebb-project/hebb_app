"use client";

import { useState } from "react";
import {
  ingestVault,
  postStimulate,
  searchVault,
  type CortexSearchResult,
} from "@/lib/cortex-api";

type Props = {
  width: number;
  live: boolean;
  onStimulate?: () => void;
};

export function VaultSearchPanel({ width, live, onStimulate }: Props) {
  const [query, setQuery] = useState("");
  const [vaultPath, setVaultPath] = useState("");
  const [results, setResults] = useState<CortexSearchResult[]>([]);
  const [selected, setSelected] = useState<CortexSearchResult | null>(null);
  const [status, setStatus] = useState("ingest a vault, then search");
  const [ingesting, setIngesting] = useState(false);
  const [pending, setPending] = useState(false);

  const runIngest = async () => {
    if (ingesting) return;
    setIngesting(true);
    setStatus(vaultPath.trim() ? "ingesting selected vault..." : "ingesting configured default vault...");
    try {
      const summary = await ingestVault(vaultPath);
      setStatus(
        `ingested ${summary.files_seen} files · ${summary.total_nodes} nodes · ${summary.total_edges} edges`,
      );
      setResults([]);
      setSelected(null);
    } catch (err) {
      setStatus(`ingest failed: ${err instanceof Error ? err.message : "unknown error"}`);
    } finally {
      setIngesting(false);
    }
  };

  const runSearch = async () => {
    const q = query.trim();
    if (!q || pending) return;
    setPending(true);
    setStatus("searching ingested vault...");
    try {
      const response = await searchVault(q, 8);
      setResults(response.results);
      setSelected(response.results[0] ?? null);
      setStatus(response.results.length ? `${response.results.length} result(s)` : "no matching nodes");
    } catch (err) {
      setStatus(`search unavailable: ${err instanceof Error ? err.message : "unknown error"}`);
      setResults([]);
      setSelected(null);
    } finally {
      setPending(false);
    }
  };

  const stimulate = async (result: CortexSearchResult) => {
    setSelected(result);
    if (!live) {
      setStatus("core live mode is off; result selected only");
      return;
    }
    try {
      await postStimulate(result.node_id, 45, 450);
      setStatus(`stimulated ${result.label}`);
      onStimulate?.();
    } catch (err) {
      setStatus(`stimulate failed: ${err instanceof Error ? err.message : "unknown error"}`);
    }
  };

  return (
    <aside className="panel vault-search" style={{ width }}>
      <div className="panel-hd mono">
        <span>vault · search</span>
        <span className="panel-hd-meta">{pending ? "running" : "generic"}</span>
      </div>
      <div className="vault-search-body">
        <div className="vault-search-copy">
          Choose an Obsidian-compatible vault folder, ingest it through the Rust
          core, then search the indexed notes. Results come from the core, not
          frontend fixtures.
        </div>
        <div className="vault-path-row">
          <input
            className="chat-input"
            value={vaultPath}
            onChange={(event) => setVaultPath(event.target.value)}
            placeholder="vault folder path, or blank for configured default"
            disabled={ingesting}
          />
          <button className="btn-send mono" onClick={runIngest} disabled={ingesting}>
            INGEST
          </button>
        </div>
        <div className="vault-search-row">
          <input
            className="chat-input"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") runSearch();
            }}
            placeholder="search labels, note bodies, paths..."
            disabled={pending}
          />
          <button className="btn-send mono" onClick={runSearch} disabled={pending || !query.trim()}>
            FIND
          </button>
        </div>
        <div className="vault-search-status mono">{status}</div>
        <div className="vault-results">
          {results.map((result, index) => (
            <button key={result.node_id} className="vault-result" onClick={() => stimulate(result)}>
              <div className="vault-result-top mono">
                <span>{String(index + 1).padStart(2, "0")} · {result.node_type}</span>
                <span>{result.score.toFixed(1)}</span>
              </div>
              <div className="vault-result-title">{result.label}</div>
              {result.snippet && <div className="vault-result-snippet">{result.snippet}</div>}
              <div className="vault-result-path mono">{result.source_file ?? "stub node"}</div>
            </button>
          ))}
        </div>
      </div>
      <div className="vault-selected">
        <div className="panel-hd mono">
          <span>selected node</span>
          <span className="panel-hd-meta">click result to fire</span>
        </div>
        {selected ? (
          <div className="vault-selected-card">
            <div className="vault-result-title">{selected.label}</div>
            <div className="vault-result-snippet">{selected.snippet ?? "No note body captured for this node."}</div>
            <div className="vault-highlight-row mono">
              {selected.highlights.map((term) => <span key={term}>{term}</span>)}
            </div>
          </div>
        ) : (
          <div className="vault-empty mono">no node selected</div>
        )}
      </div>
    </aside>
  );
}
