import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Portfolio View — Account switcher, Native LEZ + Fungible Token/ATA balances, and Receive URI modal.
Item {
    id: portfolioRoot

    property string activeAccountId: ""
    property string activePrivacy: "public"
    property string nativeBalance: "0"
    property bool isCachedOffline: false
    property var accountsList: []

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            Label {
                text: "Active Account: " + portfolioRoot.activeAccountId
                font.bold: true
            }
            Label {
                text: portfolioRoot.activePrivacy === "private" ? "[SHIELDED / ZK]" : "[PUBLIC]"
            }
            Label {
                visible: portfolioRoot.isCachedOffline
                text: "Cached (Offline)"
            }
        }

        Label {
            text: "Native Balance: " + portfolioRoot.nativeBalance + " LEZ"
            font.pixelSize: 20
            font.bold: true
        }
    }
}
