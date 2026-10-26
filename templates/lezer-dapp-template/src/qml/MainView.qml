import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Starter Basecamp ui_qml dApp using the Lezer wallet:
// connect → read a shared account's balance → propose a transfer the user approves in Lezer.
Rectangle {
    id: root
    implicitWidth: 720
    implicitHeight: 480
    color: "#0f1115"

    property string log: ""
    function append(line) { log = line + "\n" + log }

    LezerProvider {
        id: lezer
        displayName: "Lezer dApp Starter"
        onConnected: (accounts) => {
            root.append("connected: " + accounts.join(", "))
            fromBox.model = accounts
        }
        onProposalResolved: (receipt) => root.append("tx " + receipt.tx_hash + " → " + receipt.status.state)
        onFailed: (code, message) => root.append("error " + code + ": " + message)
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 10

        Label { text: "Lezer dApp starter"; color: "#e8eaed"; font.pixelSize: 18; font.bold: true }
        Button { text: lezer.busy ? "Waiting for the user…" : "1. Connect"; enabled: !lezer.busy && !lezer.sessionToken; onClicked: lezer.connect() }

        ComboBox { id: fromBox; Layout.fillWidth: true; model: [] }
        Button {
            text: "2. Read balance"
            enabled: fromBox.currentText.length > 0
            onClicked: lezer.getBalance(fromBox.currentText, function (b) { root.append(b.account_id + ": " + b.balance + " (" + b.source + ")") })
        }

        TextField { id: toField; Layout.fillWidth: true; placeholderText: "Recipient (Public/…, or a lezer:priv1… receive address)" }
        TextField { id: amountField; Layout.fillWidth: true; placeholderText: "Amount in atomic units (1 token = 1000000000)" }
        Button {
            text: "3. Propose transfer"
            enabled: !lezer.busy && fromBox.currentText.length > 0 && toField.text.length > 0 && /^[1-9]\d*$/.test(amountField.text)
            onClicked: lezer.propose({ kind: "native_transfer", from: fromBox.currentText, to: toField.text.trim(), amount: amountField.text })
        }

        TextArea { Layout.fillWidth: true; Layout.fillHeight: true; readOnly: true; text: root.log; color: "#9aa0a6"; font.family: "Menlo" }
    }
}
