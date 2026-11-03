/** An integer from 0 to 255, range-checked by the server. */
export type U8 = number;
/** An integer from 0 to 65535, range-checked by the server. */
export type U16 = number;
/** An integer from 0 to 2³² − 1, range-checked by the server. */
export type U32 = number;
/** An integer from 0 to 2⁶⁴ − 1; it crosses the wire as a decimal string. */
export type U64 = bigint;
/** An integer from −128 to 127, range-checked by the server. */
export type I8 = number;
/** An integer from −32768 to 32767, range-checked by the server. */
export type I16 = number;
/** An integer from −2³¹ to 2³¹ − 1, range-checked by the server. */
export type I32 = number;
/** An integer from −2⁶³ to 2⁶³ − 1; it crosses the wire as a decimal string. */
export type I64 = bigint;
