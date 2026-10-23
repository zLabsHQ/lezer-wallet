import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Format.js" as Fmt

// Send native tokens. The privacy mode follows from the accounts:
// public→public, public→private (shield), private→public (deshield), private→private.
Item {
    id: view
    property var store
    property string status: ""
    property string error: ""

    readonly property var fromAccount: store.accounts[fromBox.currentIndex] || null
    readonly property string toText: toField.text.trim()
    readonly property string mode: {
        if (!fromAccount || !toText.length) return ""
        var fp = fromAccount.kind === "private"
        var tp = toText.indexOf("Private/") === 0 || toText.indexOf("lezer:priv1") === 0
        return fp ? (tp ? "private" : "deshielded") : (tp ? "shielded" : "public")
    }
    readonly property string atomic: Fmt.toAtomic(amountField.text)

    ColumnLayout {
        width: Math.min(parent.width, 640)
        spacing: 10

        Label { text: "From"; color: "#9aa0a6" }
        ComboBox {
            id: fromBox
            Layout.fillWidth: true
            model: store.accounts.map(function (a) { return (a.kind === "private" ? "🔒 " : "") + Fmt.label(a) + "  " + Fmt.shortId(a.account_id) })
        }

        Label { text: "To (Public/…, an own Private/…, or a lezer:priv1… receive address)"; color: "#9aa0a6" }
        TextField { id: toField; Layout.fillWidth: true; placeholderText: "Recipient" }
        RowLayout {
            Repeater {
                model: store.accounts
                Button {
                    visible: !view.fromAccount || modelData.account_id !== view.fromAccount.account_id
                    text: "→ " + Fmt.label(modelData)
                    flat: true
                    onClicked: toField.text = modelData.account_id
                }
            }
        }

        Label { text: "Amount"; color: "#9aa0a6" }
        TextField { id: amountField; Layout.fillWidth: true; placeholderText: "0.0"; inputMethodHints: Qt.ImhFormattedNumbersOnly }

        Label {
            visible: view.mode.length > 0
            Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#fdd663"
            text: view.mode === "public" ? "Public transfer: sender, recipient and amount are visible on chain."
                : view.mode === "shielded" ? "Shielding: your public sender and the amount are visible; the recipient stays private. A proof is generated on this device (minutes)."
                : view.mode === "deshielded" ? "Deshielding: the amount and the public recipient become visible. A proof is generated on this device (minutes)."
                : "Private transfer: nothing about this transfer is visible on chain. A proof is generated on this device (minutes)."
        }

        Label { visible: view.error.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#f28b82"; text: view.error }
        Label { visible: view.status.length > 0; Layout.fillWidth: true; wrapMode: Text.Wrap; color: "#81c995"; text: view.status }

        Button {
            text: "Review & send"
            enabled: view.fromAccount && view.toText.length > 0 && view.atomic.length > 0 && !store.inFlight
            onClicked: confirm.open()
        }
    }

    Dialog {
        id: confirm
        anchors.centerIn: parent
        modal: true
        title: "Confirm " + view.mode + " transfer"
        standardButtons: Dialog.Cancel | Dialog.Ok
        Label {
            wrapMode: Text.Wrap
            width: 420
            text: "Send " + Fmt.toDisplay(view.atomic) + "\nfrom " + (view.fromAccount ? view.fromAccount.account_id : "") + "\nto " + view.toText
        }
        onAccepted: {
            view.error = ""; view.status = "Submitted to the wallet…"
            store.runJob({ op: "send", from: view.fromAccount.account_id, to: view.toText, amount: view.atomic },
                         "send", function (st) {
                if (st.state === "failed") { view.status = ""; view.error = Fmt.errorText(st.error); return }
                var r = st.result
                view.status = r.status.state === "included"
                    ? "Included in block #" + r.status.block_id + " — tx " + r.tx_hash
                    : "Submitted, not yet included — tx " + r.tx_hash
                amountField.text = ""
            })
        }
    }
}
