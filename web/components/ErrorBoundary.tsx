"use client";

import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { children: ReactNode; fallback?: (err: Error, reset: () => void) => ReactNode };
type State = { error: Error | null };

/**
 * Top-level error boundary so unhandled render errors in heavy client
 * components (ConnectomeView, ChatPanel) surface as a legible UI panel
 * instead of a blank screen + console wall. Lives in the app so the
 * dev experience is the same as prod here.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Keep going to the console in dev — useful with the React DevTools.
    console.error("[ErrorBoundary]", error, info);
  }

  reset = () => this.setState({ error: null });

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    if (this.props.fallback) return this.props.fallback(error, this.reset);
    return (
      <div
        style={{
          padding: 20,
          margin: 20,
          border: "1px solid rgba(255,93,143,0.4)",
          borderRadius: 8,
          background: "rgba(20,8,12,0.85)",
          color: "rgba(255,200,210,0.95)",
          fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
          fontSize: 13,
          maxWidth: 720,
        }}
      >
        <div style={{ fontWeight: 600, marginBottom: 6 }}>render error</div>
        <div style={{ opacity: 0.85, whiteSpace: "pre-wrap" }}>
          {error.message}
        </div>
        {error.stack && (
          <details style={{ marginTop: 8, opacity: 0.65 }}>
            <summary style={{ cursor: "pointer" }}>stack</summary>
            <pre style={{ whiteSpace: "pre-wrap", fontSize: 11, marginTop: 6 }}>
              {error.stack}
            </pre>
          </details>
        )}
        <button
          onClick={this.reset}
          style={{
            marginTop: 12, padding: "6px 12px",
            background: "rgba(125,249,255,0.18)",
            border: "1px solid rgba(125,249,255,0.5)",
            color: "rgba(125,249,255,0.95)",
            borderRadius: 4, cursor: "pointer",
          }}
        >
          retry
        </button>
      </div>
    );
  }
}
