import QtQuick

// Drop-in QML connector for Basecamp `ui_qml` dApp modules consuming `lezer_wallet_provider`.
QtObject {
    id: provider

    property string moduleName: "lezer_wallet_provider"
    property string sessionToken: ""
    property var authorizedAccounts: []

    signal connected(string sessionToken, var accounts)
    signal txProposed(string requestId, var summary)
    signal txConfirmed(string txHash)
    signal errorOccurred(string code, string message)

    function connectDapp(dappMetadataJson) {
        if (typeof logos === "undefined" || !logos) {
            provider.errorOccurred("LZR-4001", "Logos IPC bridge unavailable");
            return;
        }
        logos.callModuleAsync(provider.moduleName, "connect", [dappMetadataJson], function(raw) {
            var res = JSON.parse(raw);
            if (res.error) {
                provider.errorOccurred(res.error.code, res.error.message);
            }
        });
    }

    function proposeTransaction(intentJson) {
        if (typeof logos === "undefined" || !logos) {
            provider.errorOccurred("LZR-4001", "Logos IPC bridge unavailable");
            return;
        }
        logos.callModuleAsync(
            provider.moduleName,
            "propose_transaction",
            [provider.sessionToken, intentJson],
            function(raw) {
                var res = JSON.parse(raw);
                if (res.error) {
                    provider.errorOccurred(res.error.code, res.error.message);
                } else {
                    provider.txProposed(res.request_id, res.summary);
                }
            }
        );
    }
}
