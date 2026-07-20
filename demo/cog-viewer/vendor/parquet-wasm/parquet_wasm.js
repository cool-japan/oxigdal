let wasm;

const heap = new Array(128).fill(undefined);

heap.push(undefined, null, true, false);

let heap_next = heap.length;

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function getObject(idx) { return heap[idx]; }

function dropObject(idx) {
    if (idx < 132) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

const cachedTextDecoder = (typeof TextDecoder !== 'undefined' ? new TextDecoder('utf-8', { ignoreBOM: true, fatal: true }) : { decode: () => { throw Error('TextDecoder not available') } } );

if (typeof TextDecoder !== 'undefined') { cachedTextDecoder.decode(); };

let cachedUint8Memory0 = null;

function getUint8Memory0() {
    if (cachedUint8Memory0 === null || cachedUint8Memory0.byteLength === 0) {
        cachedUint8Memory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8Memory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return cachedTextDecoder.decode(getUint8Memory0().subarray(ptr, ptr + len));
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

let cachedFloat64Memory0 = null;

function getFloat64Memory0() {
    if (cachedFloat64Memory0 === null || cachedFloat64Memory0.byteLength === 0) {
        cachedFloat64Memory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64Memory0;
}

let cachedInt32Memory0 = null;

function getInt32Memory0() {
    if (cachedInt32Memory0 === null || cachedInt32Memory0.byteLength === 0) {
        cachedInt32Memory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32Memory0;
}

let WASM_VECTOR_LEN = 0;

const cachedTextEncoder = (typeof TextEncoder !== 'undefined' ? new TextEncoder('utf-8') : { encode: () => { throw Error('TextEncoder not available') } } );

const encodeString = (typeof cachedTextEncoder.encodeInto === 'function'
    ? function (arg, view) {
    return cachedTextEncoder.encodeInto(arg, view);
}
    : function (arg, view) {
    const buf = cachedTextEncoder.encode(arg);
    view.set(buf);
    return {
        read: arg.length,
        written: buf.length
    };
});

function passStringToWasm0(arg, malloc, realloc) {

    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8Memory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8Memory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }

    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8Memory0().subarray(ptr + offset, ptr + len);
        const ret = encodeString(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

let cachedBigInt64Memory0 = null;

function getBigInt64Memory0() {
    if (cachedBigInt64Memory0 === null || cachedBigInt64Memory0.byteLength === 0) {
        cachedBigInt64Memory0 = new BigInt64Array(wasm.memory.buffer);
    }
    return cachedBigInt64Memory0;
}

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => {
    wasm.__wbindgen_export_2.get(state.dtor)(state.a, state.b)
});

function makeMutClosure(arg0, arg1, dtor, f) {
    const state = { a: arg0, b: arg1, cnt: 1, dtor };
    const real = (...args) => {
        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            if (--state.cnt === 0) {
                wasm.__wbindgen_export_2.get(state.dtor)(a, state.b);
                CLOSURE_DTORS.unregister(state);
            } else {
                state.a = a;
            }
        }
    };
    real.original = state;
    CLOSURE_DTORS.register(real, state, state);
    return real;
}
function __wbg_adapter_50(arg0, arg1, arg2) {
    wasm._dyn_core__ops__function__FnMut__A____Output___R_as_wasm_bindgen__closure__WasmClosure___describe__invoke__h1de40baa0df51db0(arg0, arg1, addHeapObject(arg2));
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8Memory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}
/**
* Read a Parquet file into Arrow data.
*
* This returns an Arrow table in WebAssembly memory. To transfer the Arrow table to JavaScript
* memory you have two options:
*
* - (Easier): Call {@linkcode Table.intoIPCStream} to construct a buffer that can be parsed with
*   Arrow JS's `tableFromIPC` function.
* - (More performant but bleeding edge): Call {@linkcode Table.intoFFI} to construct a data
*   representation that can be parsed zero-copy from WebAssembly with
*   [arrow-js-ffi](https://github.com/kylebarron/arrow-js-ffi) using `parseTable`.
*
* Example with IPC stream:
*
* ```js
* import { tableFromIPC } from "apache-arrow";
* import initWasm, {readParquet} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
*
* const resp = await fetch("https://example.com/file.parquet");
* const parquetUint8Array = new Uint8Array(await resp.arrayBuffer());
* const arrowWasmTable = readParquet(parquetUint8Array);
* const arrowTable = tableFromIPC(arrowWasmTable.intoIPCStream());
* ```
*
* Example with `arrow-js-ffi`:
*
* ```js
* import { parseTable } from "arrow-js-ffi";
* import initWasm, {readParquet, wasmMemory} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
* const WASM_MEMORY = wasmMemory();
*
* const resp = await fetch("https://example.com/file.parquet");
* const parquetUint8Array = new Uint8Array(await resp.arrayBuffer());
* const arrowWasmTable = readParquet(parquetUint8Array);
* const ffiTable = arrowWasmTable.intoFFI();
* const arrowTable = parseTable(
*   WASM_MEMORY.buffer,
*   ffiTable.arrayAddrs(),
*   ffiTable.schemaAddr()
* );
* ```
*
* @param parquet_file Uint8Array containing Parquet data
* @param options
*
*    Options for reading Parquet data. Optional keys include:
*
*    - `batchSize`: The number of rows in each batch. If not provided, the upstream parquet
*           default is 1024.
*    - `rowGroups`: Only read data from the provided row group indexes.
*    - `limit`: Provide a limit to the number of rows to be read.
*    - `offset`: Provide an offset to skip over the given number of rows.
*    - `columns`: The column names from the file to read.
* @param {Uint8Array} parquet_file
* @param {ReaderOptions | undefined} [options]
* @returns {Table}
*/
export function readParquet(parquet_file, options) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passArray8ToWasm0(parquet_file, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.readParquet(retptr, ptr0, len0, isLikeNone(options) ? 0 : addHeapObject(options));
        var r0 = getInt32Memory0()[retptr / 4 + 0];
        var r1 = getInt32Memory0()[retptr / 4 + 1];
        var r2 = getInt32Memory0()[retptr / 4 + 2];
        if (r2) {
            throw takeObject(r1);
        }
        return Table.__wrap(r0);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
* Read an Arrow schema from a Parquet file in memory.
*
* This returns an Arrow schema in WebAssembly memory. To transfer the Arrow schema to JavaScript
* memory you have two options:
*
* - (Easier): Call {@linkcode Schema.intoIPCStream} to construct a buffer that can be parsed with
*   Arrow JS's `tableFromIPC` function. This results in an Arrow JS Table with zero rows but a
*   valid schema.
* - (More performant but bleeding edge): Call {@linkcode Schema.intoFFI} to construct a data
*   representation that can be parsed zero-copy from WebAssembly with
*   [arrow-js-ffi](https://github.com/kylebarron/arrow-js-ffi) using `parseSchema`.
*
* Example with IPC Stream:
*
* ```js
* import { tableFromIPC } from "apache-arrow";
* import initWasm, {readSchema} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
*
* const resp = await fetch("https://example.com/file.parquet");
* const parquetUint8Array = new Uint8Array(await resp.arrayBuffer());
* const arrowWasmSchema = readSchema(parquetUint8Array);
* const arrowTable = tableFromIPC(arrowWasmSchema.intoIPCStream());
* const arrowSchema = arrowTable.schema;
* ```
*
* Example with `arrow-js-ffi`:
*
* ```js
* import { parseSchema } from "arrow-js-ffi";
* import initWasm, {readSchema, wasmMemory} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
* const WASM_MEMORY = wasmMemory();
*
* const resp = await fetch("https://example.com/file.parquet");
* const parquetUint8Array = new Uint8Array(await resp.arrayBuffer());
* const arrowWasmSchema = readSchema(parquetUint8Array);
* const ffiSchema = arrowWasmSchema.intoFFI();
* const arrowTable = parseSchema(WASM_MEMORY.buffer, ffiSchema.addr());
* const arrowSchema = arrowTable.schema;
* ```
*
* @param parquet_file Uint8Array containing Parquet data
* @param {Uint8Array} parquet_file
* @returns {Schema}
*/
export function readSchema(parquet_file) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passArray8ToWasm0(parquet_file, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.readSchema(retptr, ptr0, len0);
        var r0 = getInt32Memory0()[retptr / 4 + 0];
        var r1 = getInt32Memory0()[retptr / 4 + 1];
        var r2 = getInt32Memory0()[retptr / 4 + 2];
        if (r2) {
            throw takeObject(r1);
        }
        return Schema.__wrap(r0);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
    return instance.ptr;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8Memory0().subarray(ptr / 1, ptr / 1 + len);
}
/**
* Write Arrow data to a Parquet file.
*
* For example, to create a Parquet file with Snappy compression:
*
* ```js
* import { tableToIPC } from "apache-arrow";
* // Edit the `parquet-wasm` import as necessary
* import initWasm, {
*   Table,
*   WriterPropertiesBuilder,
*   Compression,
*   writeParquet,
* } from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
*
* // Given an existing arrow JS table under `table`
* const wasmTable = Table.fromIPCStream(tableToIPC(table, "stream"));
* const writerProperties = new WriterPropertiesBuilder()
*   .setCompression(Compression.SNAPPY)
*   .build();
* const parquetUint8Array = writeParquet(wasmTable, writerProperties);
* ```
*
* If `writerProperties` is not provided or is `null`, the default writer properties will be used.
* This is equivalent to `new WriterPropertiesBuilder().build()`.
*
* @param table A {@linkcode Table} representation in WebAssembly memory.
* @param writer_properties (optional) Configuration for writing to Parquet. Use the {@linkcode
* WriterPropertiesBuilder} to build a writing configuration, then call `.build()` to create an
* immutable writer properties to pass in here.
* @returns Uint8Array containing written Parquet data.
* @param {Table} table
* @param {WriterProperties | undefined} [writer_properties]
* @returns {Uint8Array}
*/
export function writeParquet(table, writer_properties) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        _assertClass(table, Table);
        var ptr0 = table.__destroy_into_raw();
        let ptr1 = 0;
        if (!isLikeNone(writer_properties)) {
            _assertClass(writer_properties, WriterProperties);
            ptr1 = writer_properties.__destroy_into_raw();
        }
        wasm.writeParquet(retptr, ptr0, ptr1);
        var r0 = getInt32Memory0()[retptr / 4 + 0];
        var r1 = getInt32Memory0()[retptr / 4 + 1];
        var r2 = getInt32Memory0()[retptr / 4 + 2];
        var r3 = getInt32Memory0()[retptr / 4 + 3];
        if (r3) {
            throw takeObject(r2);
        }
        var v3 = getArrayU8FromWasm0(r0, r1).slice();
        wasm.__wbindgen_free(r0, r1 * 1, 1);
        return v3;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
* Read a Parquet file into a stream of Arrow `RecordBatch`es.
*
* This returns a ReadableStream containing RecordBatches in WebAssembly memory. To transfer the
* Arrow table to JavaScript memory you have two options:
*
* - (Easier): Call {@linkcode RecordBatch.intoIPCStream} to construct a buffer that can be parsed
*   with Arrow JS's `tableFromIPC` function. (The table will have a single internal record
*   batch).
* - (More performant but bleeding edge): Call {@linkcode RecordBatch.intoFFI} to construct a data
*   representation that can be parsed zero-copy from WebAssembly with
*   [arrow-js-ffi](https://github.com/kylebarron/arrow-js-ffi) using `parseRecordBatch`.
*
* Example with IPC stream:
*
* ```js
* import { tableFromIPC } from "apache-arrow";
* import initWasm, {readParquetStream} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
*
* const stream = await wasm.readParquetStream(url);
*
* const batches = [];
* for await (const wasmRecordBatch of stream) {
*   const arrowTable = tableFromIPC(wasmRecordBatch.intoIPCStream());
*   batches.push(...arrowTable.batches);
* }
* const table = new arrow.Table(batches);
* ```
*
* Example with `arrow-js-ffi`:
*
* ```js
* import { parseRecordBatch } from "arrow-js-ffi";
* import initWasm, {readParquetStream, wasmMemory} from "parquet-wasm";
*
* // Instantiate the WebAssembly context
* await initWasm();
* const WASM_MEMORY = wasmMemory();
*
* const stream = await wasm.readParquetStream(url);
*
* const batches = [];
* for await (const wasmRecordBatch of stream) {
*   const ffiRecordBatch = wasmRecordBatch.intoFFI();
*   const recordBatch = parseRecordBatch(
*     WASM_MEMORY.buffer,
*     ffiRecordBatch.arrayAddr(),
*     ffiRecordBatch.schemaAddr(),
*     true
*   );
*   batches.push(recordBatch);
* }
* const table = new arrow.Table(batches);
* ```
*
* @param url URL to Parquet file
* @param {string} url
* @param {number | undefined} [content_length]
* @returns {Promise<ReadableStream>}
*/
export function readParquetStream(url, content_length) {
    const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.readParquetStream(ptr0, len0, !isLikeNone(content_length), isLikeNone(content_length) ? 0 : content_length);
    return takeObject(ret);
}

let cachedUint32Memory0 = null;

function getUint32Memory0() {
    if (cachedUint32Memory0 === null || cachedUint32Memory0.byteLength === 0) {
        cachedUint32Memory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32Memory0;
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getUint32Memory0();
    const slice = mem.subarray(ptr / 4, ptr / 4 + len);
    const result = [];
    for (let i = 0; i < slice.length; i++) {
        result.push(takeObject(slice[i]));
    }
    return result;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        wasm.__wbindgen_exn_store(addHeapObject(e));
    }
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32Memory0().subarray(ptr / 4, ptr / 4 + len);
}
/**
* Returns a handle to this wasm instance's `WebAssembly.Memory`
* @returns {Memory}
*/
export function wasmMemory() {
    const ret = wasm.wasmMemory();
    return takeObject(ret);
}

/**
* Returns a handle to this wasm instance's `WebAssembly.Table` which is the indirect function
* table used by Rust
* @returns {FunctionTable}
*/
export function _functionTable() {
    const ret = wasm._functionTable();
    return takeObject(ret);
}

function __wbg_adapter_285(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen__convert__closures__invoke2_mut__h26b6dc7d05b06fdf(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

/**
* The Parquet version to use when writing
*/
export const WriterVersion = Object.freeze({ V1:0,"0":"V1",V2:1,"1":"V2", });
/**
* Controls the level of statistics to be computed by the writer
*/
export const EnabledStatistics = Object.freeze({
/**
* Compute no statistics
*/
None:0,"0":"None",
/**
* Compute chunk-level statistics but not page-level
*/
Chunk:1,"1":"Chunk",
/**
* Compute page-level and chunk-level statistics
*/
Page:2,"2":"Page", });
/**
* Encodings supported by Parquet.
* Not all encodings are valid for all types. These enums are also used to specify the
* encoding of definition and repetition levels.
*/
export const Encoding = Object.freeze({
/**
* Default byte encoding.
* - BOOLEAN - 1 bit per value, 0 is false; 1 is true.
* - INT32 - 4 bytes per value, stored as little-endian.
* - INT64 - 8 bytes per value, stored as little-endian.
* - FLOAT - 4 bytes per value, stored as little-endian.
* - DOUBLE - 8 bytes per value, stored as little-endian.
* - BYTE_ARRAY - 4 byte length stored as little endian, followed by bytes.
* - FIXED_LEN_BYTE_ARRAY - just the bytes are stored.
*/
PLAIN:0,"0":"PLAIN",
/**
* **Deprecated** dictionary encoding.
*
* The values in the dictionary are encoded using PLAIN encoding.
* Since it is deprecated, RLE_DICTIONARY encoding is used for a data page, and
* PLAIN encoding is used for dictionary page.
*/
PLAIN_DICTIONARY:1,"1":"PLAIN_DICTIONARY",
/**
* Group packed run length encoding.
*
* Usable for definition/repetition levels encoding and boolean values.
*/
RLE:2,"2":"RLE",
/**
* Bit packed encoding.
*
* This can only be used if the data has a known max width.
* Usable for definition/repetition levels encoding.
*/
BIT_PACKED:3,"3":"BIT_PACKED",
/**
* Delta encoding for integers, either INT32 or INT64.
*
* Works best on sorted data.
*/
DELTA_BINARY_PACKED:4,"4":"DELTA_BINARY_PACKED",
/**
* Encoding for byte arrays to separate the length values and the data.
*
* The lengths are encoded using DELTA_BINARY_PACKED encoding.
*/
DELTA_LENGTH_BYTE_ARRAY:5,"5":"DELTA_LENGTH_BYTE_ARRAY",
/**
* Incremental encoding for byte arrays.
*
* Prefix lengths are encoded using DELTA_BINARY_PACKED encoding.
* Suffixes are stored using DELTA_LENGTH_BYTE_ARRAY encoding.
*/
DELTA_BYTE_ARRAY:6,"6":"DELTA_BYTE_ARRAY",
/**
* Dictionary encoding.
*
* The ids are encoded using the RLE encoding.
*/
RLE_DICTIONARY:7,"7":"RLE_DICTIONARY",
/**
* Encoding for floating-point data.
*
* K byte-streams are created where K is the size in bytes of the data type.
* The individual bytes of an FP value are scattered to the corresponding stream and
* the streams are concatenated.
* This itself does not reduce the size of the data but can lead to better compression
* afterwards.
*/
BYTE_STREAM_SPLIT:8,"8":"BYTE_STREAM_SPLIT", });
/**
* Supported compression algorithms.
*
* Codecs added in format version X.Y can be read by readers based on X.Y and later.
* Codec support may vary between readers based on the format version and
* libraries available at runtime.
*/
export const Compression = Object.freeze({ UNCOMPRESSED:0,"0":"UNCOMPRESSED",SNAPPY:1,"1":"SNAPPY",GZIP:2,"2":"GZIP",BROTLI:3,"3":"BROTLI",
/**
* @deprecated as of Parquet 2.9.0.
* Switch to LZ4_RAW
*/
LZ4:4,"4":"LZ4",ZSTD:5,"5":"ZSTD",LZ4_RAW:6,"6":"LZ4_RAW",LZO:7,"7":"LZO", });

const ColumnChunkMetaDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_columnchunkmetadata_free(ptr >>> 0));
/**
* Metadata for a Parquet column chunk.
*/
export class ColumnChunkMetaData {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(ColumnChunkMetaData.prototype);
        obj.__wbg_ptr = ptr;
        ColumnChunkMetaDataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ColumnChunkMetaDataFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_columnchunkmetadata_free(ptr);
    }
    /**
    * File where the column chunk is stored.
    *
    * If not set, assumed to belong to the same file as the metadata.
    * This path is relative to the current file.
    * @returns {string | undefined}
    */
    filePath() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.columnchunkmetadata_filePath(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            let v1;
            if (r0 !== 0) {
                v1 = getStringFromWasm0(r0, r1).slice();
                wasm.__wbindgen_free(r0, r1 * 1, 1);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Byte offset in `file_path()`.
    * @returns {bigint}
    */
    fileOffset() {
        const ret = wasm.columnchunkmetadata_fileOffset(this.__wbg_ptr);
        return ret;
    }
    /**
    * Path (or identifier) of this column.
    * @returns {(string)[]}
    */
    columnPath() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.columnchunkmetadata_columnPath(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var v1 = getArrayJsValueFromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * All encodings used for this column.
    * @returns {any[]}
    */
    encodings() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.columnchunkmetadata_encodings(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var v1 = getArrayJsValueFromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Total number of values in this column chunk.
    * @returns {number}
    */
    numValues() {
        const ret = wasm.columnchunkmetadata_numValues(this.__wbg_ptr);
        return ret;
    }
    /**
    * Compression for this column.
    * @returns {Compression}
    */
    compression() {
        const ret = wasm.columnchunkmetadata_compression(this.__wbg_ptr);
        return ret;
    }
    /**
    * Returns the total compressed data size of this column chunk.
    * @returns {number}
    */
    compressedSize() {
        const ret = wasm.columnchunkmetadata_compressedSize(this.__wbg_ptr);
        return ret;
    }
    /**
    * Returns the total uncompressed data size of this column chunk.
    * @returns {number}
    */
    uncompressedSize() {
        const ret = wasm.columnchunkmetadata_uncompressedSize(this.__wbg_ptr);
        return ret;
    }
}

const FFIArrowArrayFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffiarrowarray_free(ptr >>> 0));
/**
*/
export class FFIArrowArray {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFIArrowArrayFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffiarrowarray_free(ptr);
    }
    /**
    * @returns {number}
    */
    addr() {
        const ret = wasm.ffiarrowarray_addr(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const FFIArrowSchemaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffiarrowschema_free(ptr >>> 0));
/**
*/
export class FFIArrowSchema {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(FFIArrowSchema.prototype);
        obj.__wbg_ptr = ptr;
        FFIArrowSchemaFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFIArrowSchemaFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffiarrowschema_free(ptr);
    }
    /**
    * Access the pointer to the
    * [`ArrowSchema`](https://arrow.apache.org/docs/format/CDataInterface.html#structure-definitions)
    * struct. This can be viewed or copied (without serialization) to an Arrow JS `Field` by
    * using [`arrow-js-ffi`](https://github.com/kylebarron/arrow-js-ffi). You can access the
    * [`WebAssembly.Memory`](https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory)
    * instance by using {@linkcode wasmMemory}.
    *
    * **Example**:
    *
    * ```ts
    * import { parseRecordBatch } from "arrow-js-ffi";
    *
    * const wasmRecordBatch: FFIRecordBatch = ...
    * const wasmMemory: WebAssembly.Memory = wasmMemory();
    *
    * // Pass `true` to copy arrays across the boundary instead of creating views.
    * const jsRecordBatch = parseRecordBatch(
    *   wasmMemory.buffer,
    *   wasmRecordBatch.arrayAddr(),
    *   wasmRecordBatch.schemaAddr(),
    *   true
    * );
    * ```
    * @returns {number}
    */
    addr() {
        const ret = wasm.ffiarrowschema_addr(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const FFIDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffidata_free(ptr >>> 0));
/**
* An Arrow array including associated field metadata.
*
* Using [`arrow-js-ffi`](https://github.com/kylebarron/arrow-js-ffi), you can view or copy Arrow
* these objects to JavaScript.
*/
export class FFIData {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFIDataFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffidata_free(ptr);
    }
    /**
    * @returns {number}
    */
    arrayAddr() {
        const ret = wasm.ffiarrowschema_addr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * @returns {number}
    */
    schema_addr() {
        const ret = wasm.ffidata_schema_addr(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const FFIRecordBatchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffirecordbatch_free(ptr >>> 0));
/**
* A representation of an Arrow RecordBatch in WebAssembly memory exposed as FFI-compatible
* structs through the Arrow C Data Interface.
*/
export class FFIRecordBatch {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(FFIRecordBatch.prototype);
        obj.__wbg_ptr = ptr;
        FFIRecordBatchFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFIRecordBatchFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffirecordbatch_free(ptr);
    }
    /**
    * Access the pointer to the
    * [`ArrowArray`](https://arrow.apache.org/docs/format/CDataInterface.html#structure-definitions)
    * struct. This can be viewed or copied (without serialization) to an Arrow JS `RecordBatch` by
    * using [`arrow-js-ffi`](https://github.com/kylebarron/arrow-js-ffi). You can access the
    * [`WebAssembly.Memory`](https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory)
    * instance by using {@linkcode wasmMemory}.
    *
    * **Example**:
    *
    * ```ts
    * import { parseRecordBatch } from "arrow-js-ffi";
    *
    * const wasmRecordBatch: FFIRecordBatch = ...
    * const wasmMemory: WebAssembly.Memory = wasmMemory();
    *
    * // Pass `true` to copy arrays across the boundary instead of creating views.
    * const jsRecordBatch = parseRecordBatch(
    *   wasmMemory.buffer,
    *   wasmRecordBatch.arrayAddr(),
    *   wasmRecordBatch.schemaAddr(),
    *   true
    * );
    * ```
    * @returns {number}
    */
    arrayAddr() {
        const ret = wasm.ffirecordbatch_arrayAddr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Access the pointer to the
    * [`ArrowSchema`](https://arrow.apache.org/docs/format/CDataInterface.html#structure-definitions)
    * struct. This can be viewed or copied (without serialization) to an Arrow JS `Field` by
    * using [`arrow-js-ffi`](https://github.com/kylebarron/arrow-js-ffi). You can access the
    * [`WebAssembly.Memory`](https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory)
    * instance by using {@linkcode wasmMemory}.
    *
    * **Example**:
    *
    * ```ts
    * import { parseRecordBatch } from "arrow-js-ffi";
    *
    * const wasmRecordBatch: FFIRecordBatch = ...
    * const wasmMemory: WebAssembly.Memory = wasmMemory();
    *
    * // Pass `true` to copy arrays across the boundary instead of creating views.
    * const jsRecordBatch = parseRecordBatch(
    *   wasmMemory.buffer,
    *   wasmRecordBatch.arrayAddr(),
    *   wasmRecordBatch.schemaAddr(),
    *   true
    * );
    * ```
    * @returns {number}
    */
    schemaAddr() {
        const ret = wasm.ffirecordbatch_schemaAddr(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const FFITableFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffitable_free(ptr >>> 0));
/**
* A representation of an Arrow Table in WebAssembly memory exposed as FFI-compatible
* structs through the Arrow C Data Interface.
*/
export class FFITable {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(FFITable.prototype);
        obj.__wbg_ptr = ptr;
        FFITableFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFITableFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffitable_free(ptr);
    }
    /**
    * Get the total number of record batches in the table
    * @returns {number}
    */
    numBatches() {
        const ret = wasm.ffitable_numBatches(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Get the pointer to one ArrowSchema FFI struct
    * @returns {number}
    */
    schemaAddr() {
        const ret = wasm.ffitable_schemaAddr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Get the pointer to one ArrowArray FFI struct for a given chunk index and column index
    *
    * Access the pointer to one
    * [`ArrowArray`](https://arrow.apache.org/docs/format/CDataInterface.html#structure-definitions)
    * struct representing one of the internal `RecordBatch`es. This can be viewed or copied (without serialization) to an Arrow JS `RecordBatch` by
    * using [`arrow-js-ffi`](https://github.com/kylebarron/arrow-js-ffi). You can access the
    * [`WebAssembly.Memory`](https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory)
    * instance by using {@linkcode wasmMemory}.
    *
    * **Example**:
    *
    * ```ts
    * import * as arrow from "apache-arrow";
    * import { parseRecordBatch } from "arrow-js-ffi";
    *
    * const wasmTable: FFITable = ...
    * const wasmMemory: WebAssembly.Memory = wasmMemory();
    *
    * const jsBatches: arrow.RecordBatch[] = []
    * for (let i = 0; i < wasmTable.numBatches(); i++) {
    *   // Pass `true` to copy arrays across the boundary instead of creating views.
    *   const jsRecordBatch = parseRecordBatch(
    *     wasmMemory.buffer,
    *     wasmTable.arrayAddr(i),
    *     wasmTable.schemaAddr(),
    *     true
    *   );
    *   jsBatches.push(jsRecordBatch);
    * }
    * const jsTable = new arrow.Table(jsBatches);
    * ```
    *
    * @param chunk number The chunk index to use
    * @returns number pointer to an ArrowArray FFI struct in Wasm memory
    * @param {number} chunk
    * @returns {number}
    */
    arrayAddr(chunk) {
        const ret = wasm.ffitable_arrayAddr(this.__wbg_ptr, chunk);
        return ret >>> 0;
    }
    /**
    * @returns {Uint32Array}
    */
    arrayAddrs() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.ffitable_arrayAddrs(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    */
    drop() {
        const ptr = this.__destroy_into_raw();
        wasm.ffitable_drop(ptr);
    }
}

const FFIVectorFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_ffivector_free(ptr >>> 0));
/**
* A chunked Arrow array including associated field metadata
*/
export class FFIVector {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FFIVectorFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_ffivector_free(ptr);
    }
    /**
    * @returns {number}
    */
    schemaAddr() {
        const ret = wasm.ffivector_schemaAddr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * @param {number} i
    * @returns {number}
    */
    arrayAddr(i) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.ffivector_arrayAddr(retptr, this.__wbg_ptr, i);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}

const FileMetaDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_filemetadata_free(ptr >>> 0));
/**
* Metadata for a Parquet file.
*/
export class FileMetaData {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(FileMetaData.prototype);
        obj.__wbg_ptr = ptr;
        FileMetaDataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        FileMetaDataFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_filemetadata_free(ptr);
    }
    /**
    * Returns version of this file.
    * @returns {number}
    */
    version() {
        const ret = wasm.filemetadata_version(this.__wbg_ptr);
        return ret;
    }
    /**
    * Returns number of rows in the file.
    * @returns {number}
    */
    numRows() {
        const ret = wasm.filemetadata_numRows(this.__wbg_ptr);
        return ret;
    }
    /**
    * String message for application that wrote this file.
    *
    * This should have the following format:
    * `<application> version <application version> (build <application build hash>)`.
    *
    * ```shell
    * parquet-mr version 1.8.0 (build 0fda28af84b9746396014ad6a415b90592a98b3b)
    * ```
    * @returns {string | undefined}
    */
    createdBy() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.filemetadata_createdBy(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            let v1;
            if (r0 !== 0) {
                v1 = getStringFromWasm0(r0, r1).slice();
                wasm.__wbindgen_free(r0, r1 * 1, 1);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Returns key_value_metadata of this file.
    * @returns {Map<any, any>}
    */
    keyValueMetadata() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.filemetadata_keyValueMetadata(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}

const IntoUnderlyingByteSourceFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_intounderlyingbytesource_free(ptr >>> 0));
/**
*/
export class IntoUnderlyingByteSource {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        IntoUnderlyingByteSourceFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_intounderlyingbytesource_free(ptr);
    }
    /**
    * @returns {string}
    */
    get type() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.intounderlyingbytesource_type(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
    * @returns {number}
    */
    get autoAllocateChunkSize() {
        const ret = wasm.intounderlyingbytesource_autoAllocateChunkSize(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * @param {ReadableByteStreamController} controller
    */
    start(controller) {
        wasm.intounderlyingbytesource_start(this.__wbg_ptr, addHeapObject(controller));
    }
    /**
    * @param {ReadableByteStreamController} controller
    * @returns {Promise<any>}
    */
    pull(controller) {
        const ret = wasm.intounderlyingbytesource_pull(this.__wbg_ptr, addHeapObject(controller));
        return takeObject(ret);
    }
    /**
    */
    cancel() {
        const ptr = this.__destroy_into_raw();
        wasm.intounderlyingbytesource_cancel(ptr);
    }
}

const IntoUnderlyingSinkFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_intounderlyingsink_free(ptr >>> 0));
/**
*/
export class IntoUnderlyingSink {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        IntoUnderlyingSinkFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_intounderlyingsink_free(ptr);
    }
    /**
    * @param {any} chunk
    * @returns {Promise<any>}
    */
    write(chunk) {
        const ret = wasm.intounderlyingsink_write(this.__wbg_ptr, addHeapObject(chunk));
        return takeObject(ret);
    }
    /**
    * @returns {Promise<any>}
    */
    close() {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.intounderlyingsink_close(ptr);
        return takeObject(ret);
    }
    /**
    * @param {any} reason
    * @returns {Promise<any>}
    */
    abort(reason) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.intounderlyingsink_abort(ptr, addHeapObject(reason));
        return takeObject(ret);
    }
}

const IntoUnderlyingSourceFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_intounderlyingsource_free(ptr >>> 0));
/**
*/
export class IntoUnderlyingSource {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(IntoUnderlyingSource.prototype);
        obj.__wbg_ptr = ptr;
        IntoUnderlyingSourceFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        IntoUnderlyingSourceFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_intounderlyingsource_free(ptr);
    }
    /**
    * @param {ReadableStreamDefaultController} controller
    * @returns {Promise<any>}
    */
    pull(controller) {
        const ret = wasm.intounderlyingsource_pull(this.__wbg_ptr, addHeapObject(controller));
        return takeObject(ret);
    }
    /**
    */
    cancel() {
        const ptr = this.__destroy_into_raw();
        wasm.intounderlyingsource_cancel(ptr);
    }
}

const ParquetFileFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_parquetfile_free(ptr >>> 0));
/**
*/
export class ParquetFile {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(ParquetFile.prototype);
        obj.__wbg_ptr = ptr;
        ParquetFileFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ParquetFileFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_parquetfile_free(ptr);
    }
    /**
    * Construct a ParquetFile from a new URL.
    *
    * @param options The options to pass into `object-store`'s [`parse_url_opts`][parse_url_opts]
    *
    * [parse_url_opts]: https://docs.rs/object_store/latest/object_store/fn.parse_url_opts.html
    * @param {string} url
    * @param {Map<any, any> | undefined} [options]
    * @returns {Promise<ParquetFile>}
    */
    static fromUrl(url, options) {
        const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.parquetfile_fromUrl(ptr0, len0, isLikeNone(options) ? 0 : addHeapObject(options));
        return takeObject(ret);
    }
    /**
    * Construct a ParquetFile from a new [Blob] or [File] handle.
    *
    * [Blob]: https://developer.mozilla.org/en-US/docs/Web/API/Blob
    * [File]: https://developer.mozilla.org/en-US/docs/Web/API/File
    *
    * Safety: Do not use this in a multi-threaded environment,
    * (transitively depends on `!Send` `web_sys::Blob`)
    * @param {Blob} handle
    * @returns {Promise<ParquetFile>}
    */
    static fromFile(handle) {
        const ret = wasm.parquetfile_fromFile(addHeapObject(handle));
        return takeObject(ret);
    }
    /**
    * @returns {ParquetMetaData}
    */
    metadata() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.parquetfile_metadata(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return ParquetMetaData.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Read from the Parquet file in an async fashion.
    *
    * @param options
    *
    *    Options for reading Parquet data. Optional keys include:
    *
    *    - `batchSize`: The number of rows in each batch. If not provided, the upstream parquet
    *           default is 1024.
    *    - `rowGroups`: Only read data from the provided row group indexes.
    *    - `limit`: Provide a limit to the number of rows to be read.
    *    - `offset`: Provide an offset to skip over the given number of rows.
    *    - `columns`: The column names from the file to read.
    * @param {ReaderOptions | undefined} [options]
    * @returns {Promise<Table>}
    */
    read(options) {
        const ret = wasm.parquetfile_read(this.__wbg_ptr, isLikeNone(options) ? 0 : addHeapObject(options));
        return takeObject(ret);
    }
    /**
    * Create a readable stream of record batches.
    *
    * Each item in the stream will be a {@linkcode RecordBatch}.
    *
    * @param options
    *
    *    Options for reading Parquet data. Optional keys include:
    *
    *    - `batchSize`: The number of rows in each batch. If not provided, the upstream parquet
    *           default is 1024.
    *    - `rowGroups`: Only read data from the provided row group indexes.
    *    - `limit`: Provide a limit to the number of rows to be read.
    *    - `offset`: Provide an offset to skip over the given number of rows.
    *    - `columns`: The column names from the file to read.
    *    - `concurrency`: The number of concurrent requests to make
    * @param {ReaderOptions | undefined} [options]
    * @returns {Promise<ReadableStream>}
    */
    stream(options) {
        const ret = wasm.parquetfile_stream(this.__wbg_ptr, isLikeNone(options) ? 0 : addHeapObject(options));
        return takeObject(ret);
    }
}

const ParquetMetaDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_parquetmetadata_free(ptr >>> 0));
/**
* Global Parquet metadata.
*/
export class ParquetMetaData {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(ParquetMetaData.prototype);
        obj.__wbg_ptr = ptr;
        ParquetMetaDataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ParquetMetaDataFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_parquetmetadata_free(ptr);
    }
    /**
    * Returns file metadata as reference.
    * @returns {FileMetaData}
    */
    fileMetadata() {
        const ret = wasm.parquetmetadata_fileMetadata(this.__wbg_ptr);
        return FileMetaData.__wrap(ret);
    }
    /**
    * Returns number of row groups in this file.
    * @returns {number}
    */
    numRowGroups() {
        const ret = wasm.parquetmetadata_numRowGroups(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Returns row group metadata for `i`th position.
    * Position should be less than number of row groups `num_row_groups`.
    * @param {number} i
    * @returns {RowGroupMetaData}
    */
    rowGroup(i) {
        const ret = wasm.parquetmetadata_rowGroup(this.__wbg_ptr, i);
        return RowGroupMetaData.__wrap(ret);
    }
    /**
    * Returns row group metadata for all row groups
    * @returns {(RowGroupMetaData)[]}
    */
    rowGroups() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.parquetmetadata_rowGroups(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var v1 = getArrayJsValueFromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}

const RecordBatchFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_recordbatch_free(ptr >>> 0));
/**
* A group of columns of equal length in WebAssembly memory with an associated {@linkcode Schema}.
*/
export class RecordBatch {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(RecordBatch.prototype);
        obj.__wbg_ptr = ptr;
        RecordBatchFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        RecordBatchFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_recordbatch_free(ptr);
    }
    /**
    * The number of rows in this RecordBatch.
    * @returns {number}
    */
    get numRows() {
        const ret = wasm.recordbatch_numRows(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * The number of columns in this RecordBatch.
    * @returns {number}
    */
    get numColumns() {
        const ret = wasm.recordbatch_numColumns(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * The {@linkcode Schema} of this RecordBatch.
    * @returns {Schema}
    */
    get schema() {
        const ret = wasm.recordbatch_schema(this.__wbg_ptr);
        return Schema.__wrap(ret);
    }
    /**
    * Export this RecordBatch to FFI structs according to the Arrow C Data Interface.
    *
    * This method **does not consume** the RecordBatch, so you must remember to call {@linkcode
    * RecordBatch.free} to release the resources. The underlying arrays are reference counted, so
    * this method does not copy data, it only prevents the data from being released.
    * @returns {FFIRecordBatch}
    */
    toFFI() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.recordbatch_toFFI(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFIRecordBatch.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Export this RecordBatch to FFI structs according to the Arrow C Data Interface.
    *
    * This method **does consume** the RecordBatch, so the original RecordBatch will be
    * inaccessible after this call. You must still call {@linkcode FFIRecordBatch.free} after
    * you've finished using the FFIRecordBatch.
    * @returns {FFIRecordBatch}
    */
    intoFFI() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.recordbatch_intoFFI(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFIRecordBatch.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Consume this RecordBatch and convert to an Arrow IPC Stream buffer
    * @returns {Uint8Array}
    */
    intoIPCStream() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.recordbatch_intoIPCStream(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            var r3 = getInt32Memory0()[retptr / 4 + 3];
            if (r3) {
                throw takeObject(r2);
            }
            var v1 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 1, 1);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Override the schema of this [`RecordBatch`]
    *
    * Returns an error if `schema` is not a superset of the current schema
    * as determined by [`Schema::contains`]
    * @param {Schema} schema
    * @returns {RecordBatch}
    */
    withSchema(schema) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            _assertClass(schema, Schema);
            var ptr0 = schema.__destroy_into_raw();
            wasm.recordbatch_withSchema(retptr, this.__wbg_ptr, ptr0);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return RecordBatch.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Return a new RecordBatch where each column is sliced
    * according to `offset` and `length`
    * @param {number} offset
    * @param {number} length
    * @returns {RecordBatch}
    */
    slice(offset, length) {
        const ret = wasm.recordbatch_slice(this.__wbg_ptr, offset, length);
        return RecordBatch.__wrap(ret);
    }
    /**
    * Returns the total number of bytes of memory occupied physically by this batch.
    * @returns {number}
    */
    getArrayMemorySize() {
        const ret = wasm.recordbatch_getArrayMemorySize(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const RowGroupMetaDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_rowgroupmetadata_free(ptr >>> 0));
/**
* Metadata for a Parquet row group.
*/
export class RowGroupMetaData {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(RowGroupMetaData.prototype);
        obj.__wbg_ptr = ptr;
        RowGroupMetaDataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        RowGroupMetaDataFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_rowgroupmetadata_free(ptr);
    }
    /**
    * Number of columns in this row group.
    * @returns {number}
    */
    numColumns() {
        const ret = wasm.rowgroupmetadata_numColumns(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Returns column chunk metadata for `i`th column.
    * @param {number} i
    * @returns {ColumnChunkMetaData}
    */
    column(i) {
        const ret = wasm.rowgroupmetadata_column(this.__wbg_ptr, i);
        return ColumnChunkMetaData.__wrap(ret);
    }
    /**
    * Returns column chunk metadata for all columns
    * @returns {(ColumnChunkMetaData)[]}
    */
    columns() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.rowgroupmetadata_columns(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var v1 = getArrayJsValueFromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Number of rows in this row group.
    * @returns {number}
    */
    numRows() {
        const ret = wasm.rowgroupmetadata_numRows(this.__wbg_ptr);
        return ret;
    }
    /**
    * Total byte size of all uncompressed column data in this row group.
    * @returns {number}
    */
    totalByteSize() {
        const ret = wasm.rowgroupmetadata_totalByteSize(this.__wbg_ptr);
        return ret;
    }
    /**
    * Total size of all compressed column data in this row group.
    * @returns {number}
    */
    compressedSize() {
        const ret = wasm.rowgroupmetadata_compressedSize(this.__wbg_ptr);
        return ret;
    }
}

const SchemaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_schema_free(ptr >>> 0));
/**
* A named collection of types that defines the column names and types in a RecordBatch or Table
* data structure.
*
* A Schema can also contain extra user-defined metadata either at the Table or Column level.
* Column-level metadata is often used to define [extension
* types](https://arrow.apache.org/docs/format/Columnar.html#extension-types).
*/
export class Schema {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Schema.prototype);
        obj.__wbg_ptr = ptr;
        SchemaFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SchemaFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_schema_free(ptr);
    }
    /**
    * Export this schema to an FFIArrowSchema object, which can be read with arrow-js-ffi.
    *
    * This method **does not consume** the Schema, so you must remember to call {@linkcode
    * Schema.free} to release the resources. The underlying arrays are reference counted, so
    * this method does not copy data, it only prevents the data from being released.
    * @returns {FFIArrowSchema}
    */
    toFFI() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.schema_toFFI(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFIArrowSchema.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Export this Table to FFI structs according to the Arrow C Data Interface.
    *
    * This method **does consume** the Table, so the original Table will be
    * inaccessible after this call. You must still call {@linkcode FFITable.free} after
    * you've finished using the FFITable.
    * @returns {FFIArrowSchema}
    */
    intoFFI() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.schema_intoFFI(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFIArrowSchema.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Consume this schema and convert to an Arrow IPC Stream buffer
    * @returns {Uint8Array}
    */
    intoIPCStream() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.schema_intoIPCStream(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            var r3 = getInt32Memory0()[retptr / 4 + 3];
            if (r3) {
                throw takeObject(r2);
            }
            var v1 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 1, 1);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Sets the metadata of this `Schema` to be `metadata` and returns a new object
    * @param {SchemaMetadata} metadata
    * @returns {Schema}
    */
    withMetadata(metadata) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.schema_withMetadata(retptr, this.__wbg_ptr, addHeapObject(metadata));
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return Schema.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Find the index of the column with the given name.
    * @param {string} name
    * @returns {number}
    */
    indexOf(name) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            wasm.schema_indexOf(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Returns an immutable reference to the Map of custom metadata key-value pairs.
    * @returns {SchemaMetadata}
    */
    metadata() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.schema_metadata(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}

const TableFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_table_free(ptr >>> 0));
/**
* A Table in WebAssembly memory conforming to the Apache Arrow spec.
*
* A Table consists of one or more {@linkcode RecordBatch} objects plus a {@linkcode Schema} that
* each RecordBatch conforms to.
*/
export class Table {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Table.prototype);
        obj.__wbg_ptr = ptr;
        TableFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        TableFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_table_free(ptr);
    }
    /**
    * Access the Table's {@linkcode Schema}.
    * @returns {Schema}
    */
    get schema() {
        const ret = wasm.table_schema(this.__wbg_ptr);
        return Schema.__wrap(ret);
    }
    /**
    * The number of batches in the Table
    * @returns {number}
    */
    get numBatches() {
        const ret = wasm.table_numBatches(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * Export this Table to FFI structs according to the Arrow C Data Interface.
    *
    * This method **does not consume** the Table, so you must remember to call {@linkcode
    * Table.free} to release the resources. The underlying arrays are reference counted, so
    * this method does not copy data, it only prevents the data from being released.
    * @returns {FFITable}
    */
    toFFI() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.table_toFFI(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFITable.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Export this Table to FFI structs according to the Arrow C Data Interface.
    *
    * This method **does consume** the Table, so the original Table will be
    * inaccessible after this call. You must still call {@linkcode FFITable.free} after
    * you've finished using the FFITable.
    * @returns {FFITable}
    */
    intoFFI() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.table_intoFFI(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return FFITable.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Consume this table and convert to an Arrow IPC Stream buffer
    * @returns {Uint8Array}
    */
    intoIPCStream() {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.table_intoIPCStream(retptr, ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            var r3 = getInt32Memory0()[retptr / 4 + 3];
            if (r3) {
                throw takeObject(r2);
            }
            var v1 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_free(r0, r1 * 1, 1);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Create a table from an Arrow IPC Stream buffer
    * @param {Uint8Array} buf
    * @returns {Table}
    */
    static fromIPCStream(buf) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArray8ToWasm0(buf, wasm.__wbindgen_malloc);
            const len0 = WASM_VECTOR_LEN;
            wasm.table_fromIPCStream(retptr, ptr0, len0);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return Table.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Returns the total number of bytes of memory occupied physically by all batches in this
    * table.
    * @returns {number}
    */
    getArrayMemorySize() {
        const ret = wasm.table_getArrayMemorySize(this.__wbg_ptr);
        return ret >>> 0;
    }
}

const WasmGetOptionsFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmgetoptions_free(ptr >>> 0));
/**
*/
export class WasmGetOptions {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmGetOptionsFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmgetoptions_free(ptr);
    }
}

const WasmObjectMetaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmobjectmeta_free(ptr >>> 0));
/**
*/
export class WasmObjectMeta {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(WasmObjectMeta.prototype);
        obj.__wbg_ptr = ptr;
        WasmObjectMetaFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    toJSON() {
        return {
            location: this.location,
            last_modified: this.last_modified,
            size: this.size,
            e_tag: this.e_tag,
            version: this.version,
        };
    }

    toString() {
        return JSON.stringify(this);
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmObjectMetaFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmobjectmeta_free(ptr);
    }
    /**
    * The full path to the object
    * @returns {string}
    */
    get location() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_wasmobjectmeta_location(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
    * The full path to the object
    * @param {string} arg0
    */
    set location(arg0) {
        const ptr0 = passStringToWasm0(arg0, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_wasmobjectmeta_location(this.__wbg_ptr, ptr0, len0);
    }
    /**
    * The last modified time
    * @returns {Date}
    */
    get last_modified() {
        const ret = wasm.__wbg_get_wasmobjectmeta_last_modified(this.__wbg_ptr);
        return takeObject(ret);
    }
    /**
    * The last modified time
    * @param {Date} arg0
    */
    set last_modified(arg0) {
        wasm.__wbg_set_wasmobjectmeta_last_modified(this.__wbg_ptr, addHeapObject(arg0));
    }
    /**
    * The size in bytes of the object
    * @returns {number}
    */
    get size() {
        const ret = wasm.__wbg_get_wasmobjectmeta_size(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
    * The size in bytes of the object
    * @param {number} arg0
    */
    set size(arg0) {
        wasm.__wbg_set_wasmobjectmeta_size(this.__wbg_ptr, arg0);
    }
    /**
    * The unique identifier for the object
    *
    * <https://datatracker.ietf.org/doc/html/rfc9110#name-etag>
    * @returns {string | undefined}
    */
    get e_tag() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_wasmobjectmeta_e_tag(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            let v1;
            if (r0 !== 0) {
                v1 = getStringFromWasm0(r0, r1).slice();
                wasm.__wbindgen_free(r0, r1 * 1, 1);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * The unique identifier for the object
    *
    * <https://datatracker.ietf.org/doc/html/rfc9110#name-etag>
    * @param {string | undefined} [arg0]
    */
    set e_tag(arg0) {
        var ptr0 = isLikeNone(arg0) ? 0 : passStringToWasm0(arg0, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_wasmobjectmeta_e_tag(this.__wbg_ptr, ptr0, len0);
    }
    /**
    * A version indicator for this object
    * @returns {string | undefined}
    */
    get version() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.__wbg_get_wasmobjectmeta_version(retptr, this.__wbg_ptr);
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            let v1;
            if (r0 !== 0) {
                v1 = getStringFromWasm0(r0, r1).slice();
                wasm.__wbindgen_free(r0, r1 * 1, 1);
            }
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * A version indicator for this object
    * @param {string | undefined} [arg0]
    */
    set version(arg0) {
        var ptr0 = isLikeNone(arg0) ? 0 : passStringToWasm0(arg0, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.__wbg_set_wasmobjectmeta_version(this.__wbg_ptr, ptr0, len0);
    }
}

const WasmObjectStoreFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmobjectstore_free(ptr >>> 0));
/**
*/
export class WasmObjectStore {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmObjectStoreFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmobjectstore_free(ptr);
    }
    /**
    * @param {string} url
    * @param {object | undefined} [options]
    */
    constructor(url, options) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            wasm.wasmobjectstore_new(retptr, ptr0, len0, isLikeNone(options) ? 0 : addHeapObject(options));
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            this.__wbg_ptr = r0 >>> 0;
            return this;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * @param {string} location
    * @param {WasmGetOptions | undefined} [options]
    * @returns {Promise<ReadableStream>}
    */
    get(location, options) {
        const ptr0 = passStringToWasm0(location, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        let ptr1 = 0;
        if (!isLikeNone(options)) {
            _assertClass(options, WasmGetOptions);
            ptr1 = options.__destroy_into_raw();
        }
        const ret = wasm.wasmobjectstore_get(this.__wbg_ptr, ptr0, len0, ptr1);
        return takeObject(ret);
    }
    /**
    * @param {string | undefined} [prefix]
    * @returns {Promise<ReadableStream>}
    */
    list(prefix) {
        var ptr0 = isLikeNone(prefix) ? 0 : passStringToWasm0(prefix, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmobjectstore_list(this.__wbg_ptr, ptr0, len0);
        return takeObject(ret);
    }
}

const WriterPropertiesFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_writerproperties_free(ptr >>> 0));
/**
* Immutable struct to hold writing configuration for `writeParquet`.
*
* Use {@linkcode WriterPropertiesBuilder} to create a configuration, then call {@linkcode
* WriterPropertiesBuilder.build} to create an instance of `WriterProperties`.
*/
export class WriterProperties {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(WriterProperties.prototype);
        obj.__wbg_ptr = ptr;
        WriterPropertiesFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WriterPropertiesFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_writerproperties_free(ptr);
    }
}

const WriterPropertiesBuilderFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_writerpropertiesbuilder_free(ptr >>> 0));
/**
* Builder to create a writing configuration for `writeParquet`
*
* Call {@linkcode build} on the finished builder to create an immputable {@linkcode WriterProperties} to pass to `writeParquet`
*/
export class WriterPropertiesBuilder {

    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(WriterPropertiesBuilder.prototype);
        obj.__wbg_ptr = ptr;
        WriterPropertiesBuilderFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WriterPropertiesBuilderFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_writerpropertiesbuilder_free(ptr);
    }
    /**
    * Returns default state of the builder.
    */
    constructor() {
        const ret = wasm.writerpropertiesbuilder_new();
        this.__wbg_ptr = ret >>> 0;
        return this;
    }
    /**
    * Finalizes the configuration and returns immutable writer properties struct.
    * @returns {WriterProperties}
    */
    build() {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_build(ptr);
        return WriterProperties.__wrap(ret);
    }
    /**
    * Sets writer version.
    * @param {WriterVersion} value
    * @returns {WriterPropertiesBuilder}
    */
    setWriterVersion(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setWriterVersion(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets data page size limit.
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setDataPageSizeLimit(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setDataPageSizeLimit(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets dictionary page size limit.
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setDictionaryPageSizeLimit(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setDictionaryPageSizeLimit(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets write batch size.
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setWriteBatchSize(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setWriteBatchSize(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets maximum number of rows in a row group.
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setMaxRowGroupSize(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setMaxRowGroupSize(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets "created by" property.
    * @param {string} value
    * @returns {WriterPropertiesBuilder}
    */
    setCreatedBy(value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setCreatedBy(ptr, ptr0, len0);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets "key_value_metadata" property.
    * @param {KeyValueMetadata} value
    * @returns {WriterPropertiesBuilder}
    */
    setKeyValueMetadata(value) {
        try {
            const ptr = this.__destroy_into_raw();
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.writerpropertiesbuilder_setKeyValueMetadata(retptr, ptr, addHeapObject(value));
            var r0 = getInt32Memory0()[retptr / 4 + 0];
            var r1 = getInt32Memory0()[retptr / 4 + 1];
            var r2 = getInt32Memory0()[retptr / 4 + 2];
            if (r2) {
                throw takeObject(r1);
            }
            return WriterPropertiesBuilder.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
    * Sets encoding for any column.
    *
    * If dictionary is not enabled, this is treated as a primary encoding for all
    * columns. In case when dictionary is enabled for any column, this value is
    * considered to be a fallback encoding for that column.
    *
    * Panics if user tries to set dictionary encoding here, regardless of dictionary
    * encoding flag being set.
    * @param {Encoding} value
    * @returns {WriterPropertiesBuilder}
    */
    setEncoding(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setEncoding(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets compression codec for any column.
    * @param {Compression} value
    * @returns {WriterPropertiesBuilder}
    */
    setCompression(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setCompression(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets flag to enable/disable dictionary encoding for any column.
    *
    * Use this method to set dictionary encoding, instead of explicitly specifying
    * encoding in `set_encoding` method.
    * @param {boolean} value
    * @returns {WriterPropertiesBuilder}
    */
    setDictionaryEnabled(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setDictionaryEnabled(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets flag to enable/disable statistics for any column.
    * @param {EnabledStatistics} value
    * @returns {WriterPropertiesBuilder}
    */
    setStatisticsEnabled(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setStatisticsEnabled(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets max statistics size for any column.
    * Applicable only if statistics are enabled.
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setMaxStatisticsSize(value) {
        const ptr = this.__destroy_into_raw();
        const ret = wasm.writerpropertiesbuilder_setMaxStatisticsSize(ptr, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets encoding for a column.
    * Takes precedence over globally defined settings.
    *
    * If dictionary is not enabled, this is treated as a primary encoding for this
    * column. In case when dictionary is enabled for this column, either through
    * global defaults or explicitly, this value is considered to be a fallback
    * encoding for this column.
    *
    * Panics if user tries to set dictionary encoding here, regardless of dictionary
    * encoding flag being set.
    * @param {string} col
    * @param {Encoding} value
    * @returns {WriterPropertiesBuilder}
    */
    setColumnEncoding(col, value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setColumnEncoding(ptr, ptr0, len0, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets compression codec for a column.
    * Takes precedence over globally defined settings.
    * @param {string} col
    * @param {Compression} value
    * @returns {WriterPropertiesBuilder}
    */
    setColumnCompression(col, value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setColumnCompression(ptr, ptr0, len0, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets flag to enable/disable dictionary encoding for a column.
    * Takes precedence over globally defined settings.
    * @param {string} col
    * @param {boolean} value
    * @returns {WriterPropertiesBuilder}
    */
    setColumnDictionaryEnabled(col, value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setColumnDictionaryEnabled(ptr, ptr0, len0, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets flag to enable/disable statistics for a column.
    * Takes precedence over globally defined settings.
    * @param {string} col
    * @param {EnabledStatistics} value
    * @returns {WriterPropertiesBuilder}
    */
    setColumnStatisticsEnabled(col, value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setColumnStatisticsEnabled(ptr, ptr0, len0, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
    /**
    * Sets max size for statistics for a column.
    * Takes precedence over globally defined settings.
    * @param {string} col
    * @param {number} value
    * @returns {WriterPropertiesBuilder}
    */
    setColumnMaxStatisticsSize(col, value) {
        const ptr = this.__destroy_into_raw();
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.writerpropertiesbuilder_setColumnMaxStatisticsSize(ptr, ptr0, len0, value);
        return WriterPropertiesBuilder.__wrap(ret);
    }
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);

            } catch (e) {
                if (module.headers.get('Content-Type') != 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else {
                    throw e;
                }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);

    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };

        } else {
            return instance;
        }
    }
}

function __wbg_get_imports() {
    const imports = {};
    imports.wbg = {};
    imports.wbg.__wbindgen_number_new = function(arg0) {
        const ret = arg0;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_columnchunkmetadata_new = function(arg0) {
        const ret = ColumnChunkMetaData.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_rowgroupmetadata_new = function(arg0) {
        const ret = RowGroupMetaData.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_parquetfile_new = function(arg0) {
        const ret = ParquetFile.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_object_drop_ref = function(arg0) {
        takeObject(arg0);
    };
    imports.wbg.__wbindgen_object_clone_ref = function(arg0) {
        const ret = getObject(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_error_new = function(arg0, arg1) {
        const ret = new Error(getStringFromWasm0(arg0, arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_is_undefined = function(arg0) {
        const ret = getObject(arg0) === undefined;
        return ret;
    };
    imports.wbg.__wbindgen_in = function(arg0, arg1) {
        const ret = getObject(arg0) in getObject(arg1);
        return ret;
    };
    imports.wbg.__wbindgen_is_bigint = function(arg0) {
        const ret = typeof(getObject(arg0)) === 'bigint';
        return ret;
    };
    imports.wbg.__wbindgen_bigint_from_u64 = function(arg0) {
        const ret = BigInt.asUintN(64, arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_jsval_eq = function(arg0, arg1) {
        const ret = getObject(arg0) === getObject(arg1);
        return ret;
    };
    imports.wbg.__wbindgen_is_object = function(arg0) {
        const val = getObject(arg0);
        const ret = typeof(val) === 'object' && val !== null;
        return ret;
    };
    imports.wbg.__wbindgen_string_new = function(arg0, arg1) {
        const ret = getStringFromWasm0(arg0, arg1);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_number_get = function(arg0, arg1) {
        const obj = getObject(arg1);
        const ret = typeof(obj) === 'number' ? obj : undefined;
        getFloat64Memory0()[arg0 / 8 + 1] = isLikeNone(ret) ? 0 : ret;
        getInt32Memory0()[arg0 / 4 + 0] = !isLikeNone(ret);
    };
    imports.wbg.__wbg_fetch_1db5b0ae726d68b5 = function(arg0) {
        const ret = fetch(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_wasmobjectmeta_new = function(arg0) {
        const ret = WasmObjectMeta.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_string_get = function(arg0, arg1) {
        const obj = getObject(arg1);
        const ret = typeof(obj) === 'string' ? obj : undefined;
        var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len1 = WASM_VECTOR_LEN;
        getInt32Memory0()[arg0 / 4 + 1] = len1;
        getInt32Memory0()[arg0 / 4 + 0] = ptr1;
    };
    imports.wbg.__wbg_fetch_bc7c8e27076a5c84 = function(arg0) {
        const ret = fetch(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_done_2ffa852272310e47 = function(arg0) {
        const ret = getObject(arg0).done;
        return ret;
    };
    imports.wbg.__wbg_value_9f6eeb1e2aab8d96 = function(arg0) {
        const ret = getObject(arg0).value;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_newwithintounderlyingsource_a03a82aa1bbbb292 = function(arg0, arg1) {
        const ret = new ReadableStream(IntoUnderlyingSource.__wrap(arg0), takeObject(arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_getReader_ab94afcb5cb7689a = function() { return handleError(function (arg0) {
        const ret = getObject(arg0).getReader();
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_sethighWaterMark_ea50ed3ec2143088 = function(arg0, arg1) {
        getObject(arg0).highWaterMark = arg1;
    };
    imports.wbg.__wbg_recordbatch_new = function(arg0) {
        const ret = RecordBatch.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_table_new = function(arg0) {
        const ret = Table.__wrap(arg0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_jsval_loose_eq = function(arg0, arg1) {
        const ret = getObject(arg0) == getObject(arg1);
        return ret;
    };
    imports.wbg.__wbindgen_boolean_get = function(arg0) {
        const v = getObject(arg0);
        const ret = typeof(v) === 'boolean' ? (v ? 1 : 0) : 2;
        return ret;
    };
    imports.wbg.__wbindgen_as_number = function(arg0) {
        const ret = +getObject(arg0);
        return ret;
    };
    imports.wbg.__wbg_String_b9412f8799faab3e = function(arg0, arg1) {
        const ret = String(getObject(arg1));
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getInt32Memory0()[arg0 / 4 + 1] = len1;
        getInt32Memory0()[arg0 / 4 + 0] = ptr1;
    };
    imports.wbg.__wbg_getwithrefkey_edc2c8960f0f1191 = function(arg0, arg1) {
        const ret = getObject(arg0)[getObject(arg1)];
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_set_f975102236d3c502 = function(arg0, arg1, arg2) {
        getObject(arg0)[takeObject(arg1)] = takeObject(arg2);
    };
    imports.wbg.__wbindgen_cb_drop = function(arg0) {
        const obj = takeObject(arg0).original;
        if (obj.cnt-- == 1) {
            obj.a = 0;
            return true;
        }
        const ret = false;
        return ret;
    };
    imports.wbg.__wbg_queueMicrotask_481971b0d87f3dd4 = function(arg0) {
        queueMicrotask(getObject(arg0));
    };
    imports.wbg.__wbg_queueMicrotask_3cbae2ec6b6cd3d6 = function(arg0) {
        const ret = getObject(arg0).queueMicrotask;
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_is_function = function(arg0) {
        const ret = typeof(getObject(arg0)) === 'function';
        return ret;
    };
    imports.wbg.__wbg_fetch_921fad6ef9e883dd = function(arg0, arg1) {
        const ret = getObject(arg0).fetch(getObject(arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_new_ab6fd82b10560829 = function() { return handleError(function () {
        const ret = new Headers();
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_append_7bfcb4937d1d5e29 = function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
        getObject(arg0).append(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
    }, arguments) };
    imports.wbg.__wbg_close_a994f9425dab445c = function() { return handleError(function (arg0) {
        getObject(arg0).close();
    }, arguments) };
    imports.wbg.__wbg_enqueue_ea194723156c0cc2 = function() { return handleError(function (arg0, arg1) {
        getObject(arg0).enqueue(getObject(arg1));
    }, arguments) };
    imports.wbg.__wbg_size_9c7e57fbd4f0f4b5 = function(arg0) {
        const ret = getObject(arg0).size;
        return ret;
    };
    imports.wbg.__wbg_arrayBuffer_307ddd1bd1d04e23 = function(arg0) {
        const ret = getObject(arg0).arrayBuffer();
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_slice_1991e6e71a6587f3 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = getObject(arg0).slice(arg1, arg2);
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_read_e7d0f8a49be01d86 = function(arg0) {
        const ret = getObject(arg0).read();
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_releaseLock_5c49db976c08b864 = function(arg0) {
        getObject(arg0).releaseLock();
    };
    imports.wbg.__wbg_cancel_6ee33d4006737aef = function(arg0) {
        const ret = getObject(arg0).cancel();
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_byobRequest_72fca99f9c32c193 = function(arg0) {
        const ret = getObject(arg0).byobRequest;
        return isLikeNone(ret) ? 0 : addHeapObject(ret);
    };
    imports.wbg.__wbg_close_184931724d961ccc = function() { return handleError(function (arg0) {
        getObject(arg0).close();
    }, arguments) };
    imports.wbg.__wbg_instanceof_Response_849eb93e75734b6e = function(arg0) {
        let result;
        try {
            result = getObject(arg0) instanceof Response;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_url_5f6dc4009ac5f99d = function(arg0, arg1) {
        const ret = getObject(arg1).url;
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getInt32Memory0()[arg0 / 4 + 1] = len1;
        getInt32Memory0()[arg0 / 4 + 0] = ptr1;
    };
    imports.wbg.__wbg_status_61a01141acd3cf74 = function(arg0) {
        const ret = getObject(arg0).status;
        return ret;
    };
    imports.wbg.__wbg_headers_9620bfada380764a = function(arg0) {
        const ret = getObject(arg0).headers;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_body_9545a94f397829db = function(arg0) {
        const ret = getObject(arg0).body;
        return isLikeNone(ret) ? 0 : addHeapObject(ret);
    };
    imports.wbg.__wbg_arrayBuffer_29931d52c7206b02 = function() { return handleError(function (arg0) {
        const ret = getObject(arg0).arrayBuffer();
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_signal_a61f78a3478fd9bc = function(arg0) {
        const ret = getObject(arg0).signal;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_new_0d76b0581eca6298 = function() { return handleError(function () {
        const ret = new AbortController();
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_abort_2aa7521d5690750e = function(arg0) {
        getObject(arg0).abort();
    };
    imports.wbg.__wbg_view_7f0ce470793a340f = function(arg0) {
        const ret = getObject(arg0).view;
        return isLikeNone(ret) ? 0 : addHeapObject(ret);
    };
    imports.wbg.__wbg_respond_b1a43b2e3a06d525 = function() { return handleError(function (arg0, arg1) {
        getObject(arg0).respond(arg1 >>> 0);
    }, arguments) };
    imports.wbg.__wbg_newwithstrandinit_3fd6fba4083ff2d0 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = new Request(getStringFromWasm0(arg0, arg1), getObject(arg2));
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_get_bd8e338fbd5f5cc8 = function(arg0, arg1) {
        const ret = getObject(arg0)[arg1 >>> 0];
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_length_cd7af8117672b8b8 = function(arg0) {
        const ret = getObject(arg0).length;
        return ret;
    };
    imports.wbg.__wbg_newnoargs_e258087cd0daa0ea = function(arg0, arg1) {
        const ret = new Function(getStringFromWasm0(arg0, arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_new_d9bc3a0147634640 = function() {
        const ret = new Map();
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_next_40fc327bfc8770e6 = function(arg0) {
        const ret = getObject(arg0).next;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_next_196c84450b364254 = function() { return handleError(function (arg0) {
        const ret = getObject(arg0).next();
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_done_298b57d23c0fc80c = function(arg0) {
        const ret = getObject(arg0).done;
        return ret;
    };
    imports.wbg.__wbg_value_d93c65011f51a456 = function(arg0) {
        const ret = getObject(arg0).value;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_iterator_2cee6dadfd956dfa = function() {
        const ret = Symbol.iterator;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_get_e3c254076557e348 = function() { return handleError(function (arg0, arg1) {
        const ret = Reflect.get(getObject(arg0), getObject(arg1));
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_call_27c0f87801dedf93 = function() { return handleError(function (arg0, arg1) {
        const ret = getObject(arg0).call(getObject(arg1));
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_new_72fb9a18b5ae2624 = function() {
        const ret = new Object();
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_is_string = function(arg0) {
        const ret = typeof(getObject(arg0)) === 'string';
        return ret;
    };
    imports.wbg.__wbg_self_ce0dbfc45cf2f5be = function() { return handleError(function () {
        const ret = self.self;
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_window_c6fb939a7f436783 = function() { return handleError(function () {
        const ret = window.window;
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_globalThis_d1e6af4856ba331b = function() { return handleError(function () {
        const ret = globalThis.globalThis;
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_global_207b558942527489 = function() { return handleError(function () {
        const ret = global.global;
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_isArray_2ab64d95e09ea0ae = function(arg0) {
        const ret = Array.isArray(getObject(arg0));
        return ret;
    };
    imports.wbg.__wbg_instanceof_ArrayBuffer_836825be07d4c9d2 = function(arg0) {
        let result;
        try {
            result = getObject(arg0) instanceof ArrayBuffer;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_new_28c511d9baebfa89 = function(arg0, arg1) {
        const ret = new Error(getStringFromWasm0(arg0, arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_call_b3ca7c6051f9bec1 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = getObject(arg0).call(getObject(arg1), getObject(arg2));
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_set_8417257aaedc936b = function(arg0, arg1, arg2) {
        const ret = getObject(arg0).set(getObject(arg1), getObject(arg2));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_isSafeInteger_f7b04ef02296c4d2 = function(arg0) {
        const ret = Number.isSafeInteger(getObject(arg0));
        return ret;
    };
    imports.wbg.__wbg_new_cf3ec55744a78578 = function(arg0) {
        const ret = new Date(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_entries_95cc2c823b285a09 = function(arg0) {
        const ret = Object.entries(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_new_81740750da40724f = function(arg0, arg1) {
        try {
            var state0 = {a: arg0, b: arg1};
            var cb0 = (arg0, arg1) => {
                const a = state0.a;
                state0.a = 0;
                try {
                    return __wbg_adapter_285(a, state0.b, arg0, arg1);
                } finally {
                    state0.a = a;
                }
            };
            const ret = new Promise(cb0);
            return addHeapObject(ret);
        } finally {
            state0.a = state0.b = 0;
        }
    };
    imports.wbg.__wbg_resolve_b0083a7967828ec8 = function(arg0) {
        const ret = Promise.resolve(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_catch_0260e338d10f79ae = function(arg0, arg1) {
        const ret = getObject(arg0).catch(getObject(arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_then_0c86a60e8fcfe9f6 = function(arg0, arg1) {
        const ret = getObject(arg0).then(getObject(arg1));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_then_a73caa9a87991566 = function(arg0, arg1, arg2) {
        const ret = getObject(arg0).then(getObject(arg1), getObject(arg2));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_buffer_12d079cc21e14bdb = function(arg0) {
        const ret = getObject(arg0).buffer;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_newwithbyteoffsetandlength_aa4a17c33a06e5cb = function(arg0, arg1, arg2) {
        const ret = new Uint8Array(getObject(arg0), arg1 >>> 0, arg2 >>> 0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_new_63b92bc8671ed464 = function(arg0) {
        const ret = new Uint8Array(getObject(arg0));
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_set_a47bac70306a19a7 = function(arg0, arg1, arg2) {
        getObject(arg0).set(getObject(arg1), arg2 >>> 0);
    };
    imports.wbg.__wbg_length_c20a40f15020d68a = function(arg0) {
        const ret = getObject(arg0).length;
        return ret;
    };
    imports.wbg.__wbg_instanceof_Uint8Array_2b3bbecd033d19f6 = function(arg0) {
        let result;
        try {
            result = getObject(arg0) instanceof Uint8Array;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_newwithlength_e9b4878cebadb3d3 = function(arg0) {
        const ret = new Uint8Array(arg0 >>> 0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_newwithbyteoffset_27c6424791adc775 = function(arg0, arg1) {
        const ret = new Uint8Array(getObject(arg0), arg1 >>> 0);
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_buffer_dd7f74bc60f1faab = function(arg0) {
        const ret = getObject(arg0).buffer;
        return addHeapObject(ret);
    };
    imports.wbg.__wbg_byteLength_58f7b4fab1919d44 = function(arg0) {
        const ret = getObject(arg0).byteLength;
        return ret;
    };
    imports.wbg.__wbg_byteOffset_81d60f7392524f62 = function(arg0) {
        const ret = getObject(arg0).byteOffset;
        return ret;
    };
    imports.wbg.__wbg_stringify_8887fe74e1c50d81 = function() { return handleError(function (arg0) {
        const ret = JSON.stringify(getObject(arg0));
        return addHeapObject(ret);
    }, arguments) };
    imports.wbg.__wbg_has_0af94d20077affa2 = function() { return handleError(function (arg0, arg1) {
        const ret = Reflect.has(getObject(arg0), getObject(arg1));
        return ret;
    }, arguments) };
    imports.wbg.__wbg_set_1f9b04f170055d33 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = Reflect.set(getObject(arg0), getObject(arg1), getObject(arg2));
        return ret;
    }, arguments) };
    imports.wbg.__wbindgen_bigint_get_as_i64 = function(arg0, arg1) {
        const v = getObject(arg1);
        const ret = typeof(v) === 'bigint' ? v : undefined;
        getBigInt64Memory0()[arg0 / 8 + 1] = isLikeNone(ret) ? BigInt(0) : ret;
        getInt32Memory0()[arg0 / 4 + 0] = !isLikeNone(ret);
    };
    imports.wbg.__wbindgen_debug_string = function(arg0, arg1) {
        const ret = debugString(getObject(arg1));
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getInt32Memory0()[arg0 / 4 + 1] = len1;
        getInt32Memory0()[arg0 / 4 + 0] = ptr1;
    };
    imports.wbg.__wbindgen_throw = function(arg0, arg1) {
        throw new Error(getStringFromWasm0(arg0, arg1));
    };
    imports.wbg.__wbindgen_memory = function() {
        const ret = wasm.memory;
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_function_table = function() {
        const ret = wasm.__wbindgen_export_2;
        return addHeapObject(ret);
    };
    imports.wbg.__wbindgen_closure_wrapper13295 = function(arg0, arg1, arg2) {
        const ret = makeMutClosure(arg0, arg1, 3248, __wbg_adapter_50);
        return addHeapObject(ret);
    };

    return imports;
}

function __wbg_init_memory(imports, maybe_memory) {

}

function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    __wbg_init.__wbindgen_wasm_module = module;
    cachedBigInt64Memory0 = null;
    cachedFloat64Memory0 = null;
    cachedInt32Memory0 = null;
    cachedUint32Memory0 = null;
    cachedUint8Memory0 = null;


    return wasm;
}

function initSync(module) {
    if (wasm !== undefined) return wasm;

    const imports = __wbg_get_imports();

    __wbg_init_memory(imports);

    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }

    const instance = new WebAssembly.Instance(module, imports);

    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(input) {
    if (wasm !== undefined) return wasm;

    if (typeof input === 'undefined') {
        input = new URL('parquet_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof input === 'string' || (typeof Request === 'function' && input instanceof Request) || (typeof URL === 'function' && input instanceof URL)) {
        input = fetch(input);
    }

    __wbg_init_memory(imports);

    const { instance, module } = await __wbg_load(await input, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync }
export default __wbg_init;
