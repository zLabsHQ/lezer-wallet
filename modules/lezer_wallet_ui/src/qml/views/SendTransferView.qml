import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Send & Privacy Bridge View — Public, Shield, Deshield & Private transfer composer.
Item {
    id: sendRoot

    signal sendRequested(string fromAccount, string toAccount, string amount, string assetId)

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: "Send Native LEZ or Fungible Token (ATA)"
            font.bold: true
        }

        TextField {
            id: fromField
            placeholderText: "From Account (e.g. Main Public or alice)"
            Layout.fillWidth: true
        }

        TextField {
            id: toField
            placeholderText: "Recipient (Public/<base58>, Private/<base58>, or lezer:priv1...)"
            Layout.fillWidth: true
        }

        TextField {
            id: amountField
            placeholderText: "Amount"
            Layout.fillWidth: true
        }

        Button {
            text: "Review & Propose Transfer"
            onClicked: sendRoot.sendRequested(fromField.text, toField.text, amountField.text, "native")
        }
    }
}
