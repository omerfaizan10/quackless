"use client";

import { useEffect, useRef } from "react";
import { EditorView, basicSetup } from "codemirror";
import { keymap } from "@codemirror/view";
import { EditorState, Prec } from "@codemirror/state";
import { sql, PostgreSQL } from "@codemirror/lang-sql";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

// Listing style from a printed systems paper: bold keywords, everything else in ink.
const listing = HighlightStyle.define([
  { tag: t.keyword, fontWeight: "600", color: "#111214" },
  { tag: [t.string, t.special(t.string)], color: "#393d43" },
  { tag: [t.number, t.bool, t.null], color: "#111214" },
  { tag: t.comment, color: "#5b6067", fontStyle: "italic" },
  { tag: [t.operator, t.punctuation], color: "#393d43" },
  { tag: t.typeName, fontWeight: "600" },
]);

const theme = EditorView.theme({
  "&": { backgroundColor: "#ffffff", color: "#111214" },
  ".cm-content": { fontFamily: "var(--mono)", padding: "8px 0" },
  ".cm-gutters": { backgroundColor: "#ffffff", color: "#5b6067", border: "none", borderRight: "0.75px solid #dcdcd6" },
  ".cm-activeLine": { backgroundColor: "#f7f7f3" },
  ".cm-activeLineGutter": { backgroundColor: "#f7f7f3", color: "#111214" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": { backgroundColor: "#dfe7fb !important" },
  ".cm-cursor": { borderLeftColor: "#111214" },
  ".cm-matchingBracket": { backgroundColor: "#eaf0fd", outline: "none" },
});

export default function Editor({ value, onChange, onRun }: { value: string; onChange: (v: string) => void; onRun: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const runRef = useRef(onRun);
  const changeRef = useRef(onChange);
  useEffect(() => {
    runRef.current = onRun;
    changeRef.current = onChange;
  });

  useEffect(() => {
    if (!host.current) return;
    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          Prec.highest(keymap.of([{ key: "Mod-Enter", run: () => (runRef.current(), true) }])),
          basicSetup,
          sql({ dialect: PostgreSQL, upperCaseKeywords: true }),
          syntaxHighlighting(listing),
          theme,
          EditorView.lineWrapping,
          EditorView.contentAttributes.of({ "aria-label": "SQL query" }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) changeRef.current(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    return () => v.destroy();
    // the editor is created once; outside changes arrive through the effect below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
  }, [value]);

  return <div className="editor" ref={host} />;
}
