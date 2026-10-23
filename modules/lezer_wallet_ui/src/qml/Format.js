.pragma library

// LEZ native token: 10^9 atomic units per token.
var DECIMALS = 9

// The Logos QML bridge serializes a `tstr` return as a JSON string literal, so the
// payload is JSON of a string that itself holds JSON. Errors arrive as {"error": ...}.
function decode(payload) {
    var v
    try { v = JSON.parse(payload) } catch (e) { return { error: { code: "BRIDGE", message: String(payload) } } }
    if (typeof v === "string") {
        try { v = JSON.parse(v) } catch (e2) { return { error: { code: "BRIDGE", message: v } } }
    }
    if (v && typeof v.error === "string") v.error = { code: "BRIDGE", message: v.error }
    return v
}

// "1234500000" -> "1.2345" for a token with `decimals` fractional digits (exact).
function toDisplayD(atomic, decimals) {
    var s = String(atomic || "0").replace(/^0+(?=\d)/, "")
    if (!/^\d+$/.test(s)) return s
    while (s.length <= decimals) s = "0" + s
    var whole = s.slice(0, s.length - decimals)
    var frac = s.slice(s.length - decimals).replace(/0+$/, "")
    return frac.length ? whole + "." + frac : whole
}

// "1234500000" -> "1.2345"  (exact, string based, native 10^9 units)
function toDisplay(atomic) {
    var s = String(atomic || "0").replace(/^0+(?=\d)/, "")
    if (!/^\d+$/.test(s)) return s
    while (s.length <= DECIMALS) s = "0" + s
    var whole = s.slice(0, s.length - DECIMALS)
    var frac = s.slice(s.length - DECIMALS).replace(/0+$/, "")
    return frac.length ? whole + "." + frac : whole
}

// "1.2345" -> "1234500000" for a token with `decimals` fractional digits; "" if invalid.
function toAtomicD(text, decimals) {
    var t = String(text || "").trim()
    if (!/^\d+(\.\d+)?$/.test(t)) return ""
    var parts = t.split(".")
    var frac = parts.length > 1 ? parts[1] : ""
    if (frac.length > decimals) return ""
    while (frac.length < decimals) frac += "0"
    var s = (parts[0] + frac).replace(/^0+/, "")
    return s.length ? s : ""
}

// native units (10^9) — see toAtomicD for token-specific decimals.
function toAtomic(text) {
    var t = String(text || "").trim()
    if (!/^\d+(\.\d+)?$/.test(t)) return ""
    var parts = t.split(".")
    var frac = parts.length > 1 ? parts[1] : ""
    if (frac.length > DECIMALS) return ""
    while (frac.length < DECIMALS) frac += "0"
    var s = (parts[0] + frac).replace(/^0+/, "")
    return s.length ? s : ""
}

function shortId(id) {
    if (!id) return ""
    if (id.indexOf("lezer:priv1") === 0) return id.slice(0, 18) + "…"
    var i = id.indexOf("/")
    var body = i >= 0 ? id.slice(i + 1) : id
    return (i >= 0 ? id.slice(0, i + 1) : "") + (body.length > 12 ? body.slice(0, 6) + "…" + body.slice(-4) : body)
}

function label(account) {
    if (!account) return ""
    if (account.labels && account.labels.length) return account.labels[0]
    if (account.received_via) return account.received_via + " (received)"
    return shortId(account.account_id)
}

function errorText(err) {
    if (!err) return ""
    return (err.code ? err.code + ": " : "") + (err.message || "")
}
