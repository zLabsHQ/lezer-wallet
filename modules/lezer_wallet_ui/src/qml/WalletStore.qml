import QtQuick
import "Format.js" as Fmt

// Single source of UI state: the WalletStateSnapshot produced by lezer_wallet_provider.
// The UI never derives wallet state on its own; it renders the snapshot and sends ops.
QtObject {
    id: store

    readonly property string providerModule: "lezer_wallet_provider"
    property var snapshot: null
    property string lockState: snapshot ? snapshot.lock_state : "unknown"
    property var accounts: snapshot ? snapshot.accounts : []
    property var pendingRequests: snapshot ? snapshot.pending_requests : []
    property var sessions: snapshot ? snapshot.active_sessions : []
    property var activity: snapshot ? snapshot.recent_activity : []
    property var inFlight: snapshot ? snapshot.in_flight : null
    property bool providerReachable: false
    property string lastError: ""
    property var jobs: ({})   // job_id -> { description, onDone }

    signal jobFinished(string jobId, var state)

    function call(op, onOk, onErr, timeoutMs) {
        if (typeof logos === "undefined" || !logos) {
            store.lastError = "Logos bridge unavailable"
            if (onErr) onErr({ code: "BRIDGE", message: store.lastError })
            return
        }
        logos.callModuleAsync(providerModule, "wallet_call", [JSON.stringify(op)], function (payload) {
            var r = Fmt.decode(payload)
            if (r && r.error) {
                store.lastError = Fmt.errorText(r.error)
                if (onErr) onErr(r.error)
            } else {
                store.providerReachable = true
                store.lastError = ""
                if (onOk) onOk(r)
            }
        }, timeoutMs || 30000)
    }

    function refresh() {
        call({ op: "snapshot" }, function (s) { store.snapshot = s }, function () {})
    }

    // Long operations return a job id; poll until done.
    function runJob(op, description, onDone) {
        call(op, function (r) {
            if (!r || !r.job_id) { if (onDone) onDone({ state: "done", result: r }); return }
            var j = store.jobs
            j[r.job_id] = { description: description, onDone: onDone }
            store.jobs = j
            jobTimer.start()
        }, function (e) { if (onDone) onDone({ state: "failed", error: e }) })
    }

    function pollJobs() {
        var ids = Object.keys(store.jobs)
        if (ids.length === 0) { jobTimer.stop(); return }
        ids.forEach(function (id) {
            call({ op: "job", job_id: id }, function (st) {
                if (!st || st.state === "running") return
                var entry = store.jobs[id]
                var j = store.jobs
                delete j[id]
                store.jobs = j
                if (st.state === "failed") store.lastError = (entry ? entry.description + " failed — " : "") + Fmt.errorText(st.error)
                if (entry && entry.onDone) entry.onDone(st)
                store.jobFinished(id, st)
                store.refresh()
            })
        })
    }

    property Timer snapshotTimer: Timer {
        interval: 2500; repeat: true; running: true; triggeredOnStart: true
        onTriggered: store.refresh()
    }
    property Timer jobTimer: Timer {
        interval: 1500; repeat: true; running: false
        onTriggered: store.pollJobs()
    }
}
