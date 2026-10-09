import QtQuick

// Thin IPC client calling `lezer_wallet_provider` (holds zero state/keys).
QtObject {
    id: bridge

    property string moduleName: "lezer_wallet_provider"
    property string uiControlToken: "lzr_ui_ctrl_default"

    signal snapshotReceived(var snapshot)
    signal bridgeError(string code, string message)

    function callPrivileged(opObject, callback) {
        if (typeof logos === "undefined" || !logos) {
            bridge.bridgeError("LZR-4001", "Logos IPC unavailable");
            return;
        }
        var payload = JSON.stringify(opObject);
        logos.callModuleAsync(bridge.moduleName, "execute_privileged", [bridge.uiControlToken, payload], function(raw) {
            var parsed = JSON.parse(raw);
            if (parsed && parsed.error) {
                bridge.bridgeError(parsed.error.code, parsed.error.message);
            } else if (typeof callback === "function") {
                callback(parsed);
            }
        });
    }
}
