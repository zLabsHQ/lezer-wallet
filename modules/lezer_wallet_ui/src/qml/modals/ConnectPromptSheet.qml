import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Connect Prompt Sheet — Account selection + opt-in per-account private balance toggle.
Item {
    id: connectSheet

    property string requestId: ""
    property string dappName: ""
    property bool allowPrivateBalance: false

    signal approveConnect(string requestId, var selectedAccounts, var allowPrivateBalances)
    signal rejectConnect(string requestId)

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: "LEZER WALLET — CONNECTION REQUEST (" + connectSheet.dappName + ")"
            font.bold: true
        }

        CheckBox {
            id: privToggle
            text: "Allow this dApp to read my Shielded Vault balance (Opt-in ReadPrivateBalance)"
            checked: connectSheet.allowPrivateBalance
            onCheckedChanged: connectSheet.allowPrivateBalance = checked
        }

        RowLayout {
            spacing: 8
            Button {
                text: "Deny Connection"
                onClicked: connectSheet.rejectConnect(connectSheet.requestId)
            }
            Button {
                text: "Approve & Connect"
                onClicked: connectSheet.approveConnect(connectSheet.requestId, [], [])
            }
        }
    }
}
