import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// LP-0021 reference faucet mini-app: connects to Lezer Wallet through LezerProvider,
// lets the user select which shared public account receives 50 LEZ from the on-chain
// `lezer_faucet` program, and surfaces on-chain rate-limiting clearly.
Rectangle {
    id: root
    implicitWidth: 760
    implicitHeight: 540
    color: "#0f1115"

    property string status: ""
    property string error: ""
    property string currentBalance: "—"

    function refreshSelectedBalance() {
        if (wallet.sessionToken === "" || recipientBox.currentText.length === 0) return
        wallet.getBalance(recipientBox.currentText, function (b) {
            if (b && b.balance !== undefined) root.currentBalance = String(b.balance) + " atomic LEZ"
        })
    }

    LezerProvider {
        id: wallet
        displayName: "Lezer Faucet"
        onConnected: (accounts) => {
            root.error = ""
            root.status = "Connected. Select the public account to fund from the on-chain faucet."
            recipientBox.model = accounts.filter(function (a) { return a.indexOf("Public/") === 0 })
            root.refreshSelectedBalance()
        }
        onProposalResolved: (receipt) => {
            root.error = ""
            root.status = receipt.status.state === "included"
                ? "Dispensed 50 LEZ in block #" + receipt.status.block_id + " (tx " + receipt.tx_hash.slice(0, 16) + "…)."
                : "Claim submitted (tx " + receipt.tx_hash.slice(0, 16) + "…), waiting for inclusion."
            root.refreshSelectedBalance()
        }
        onFailed: (code, message) => {
            root.status = ""
            var lower = String(message).toLowerCase()
            if (lower.indexOf("already claimed") >= 0 || lower.indexOf("rate limit") >= 0) {
                root.error = "Rate limit (" + code + "): this account has already claimed its 50 LEZ allocation. Select another public account."
            } else {
                root.error = code + ": " + message
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 12

        Label { text: "Lezer Zone Faucet"; color: "#e8eaed"; font.pixelSize: 22; font.bold: true }
        Label {
            Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#9aa0a6"
            text: "Claims 50 LEZ (50,000,000,000 atomic units) directly from the on-chain lezer_faucet treasury PDA via custody_transfer. Each public account can claim once (enforced on-chain by its deterministic claim-ticket PDA)."
        }

        Button {
            visible: wallet.sessionToken === ""
            text: wallet.busy ? "Waiting for approval in Lezer Wallet…" : "Connect Lezer Wallet"
            enabled: !wallet.busy
            onClicked: wallet.connect()
        }

        ColumnLayout {
            visible: wallet.sessionToken !== ""
            Layout.fillWidth: true
            spacing: 10

            Label { text: "Recipient public account"; color: "#9aa0a6" }
            ComboBox {
                id: recipientBox
                Layout.fillWidth: true
                model: []
                onCurrentTextChanged: root.refreshSelectedBalance()
            }

            RowLayout {
                spacing: 8
                Label { text: "Current balance:"; color: "#9aa0a6" }
                Label { text: root.currentBalance; color: "#e8eaed"; font.bold: true }
                Button {
                    text: "Refresh"
                    enabled: !wallet.busy && recipientBox.currentText.length > 0
                    onClicked: root.refreshSelectedBalance()
                }
            }

            Button {
                text: wallet.busy ? "Waiting for approval…" : "Claim 50 LEZ from On-Chain Faucet"
                highlighted: true
                enabled: !wallet.busy && recipientBox.currentText.length > 0
                onClicked: {
                    root.error = ""
                    root.status = "Review and approve the faucet claim in Lezer Wallet."
                    wallet.propose({ kind: "faucet_claim", recipient: recipientBox.currentText })
                }
            }
        }

        Label { visible: root.status.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#81c995"; text: root.status }
        Label { visible: root.error.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"; text: root.error }
        Item { Layout.fillHeight: true }
    }
}
