/* tslint:disable */
/* eslint-disable */

export class Engine {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Run DDL (CREATE TABLE ...) or any statements; returns nothing on success.
     */
    execute_script(sql: string): void;
    /**
     * Load CSV bytes (with a header row) into an existing table. Returns the row count.
     */
    load_csv(table: string, bytes: Uint8Array | number[]): number;
    constructor();
    /**
     * Run one statement. Returns the result, plans, per-operator stats and the
     * execution trace as JSON (at most `max_rows` rows).
     */
    query(sql: string, optimize: boolean, max_rows: number): string;
    /**
     * Tables with row counts and column types, as JSON.
     */
    tables(): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_engine_free: (a: number, b: number) => void;
    readonly engine_execute_script: (a: number, b: number, c: number) => [number, number];
    readonly engine_load_csv: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly engine_new: () => number;
    readonly engine_query: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly engine_tables: (a: number) => [number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
