import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "views"
import "modals"

Rectangle {
    id: root
    implicitWidth: 980
    implicitHeight: 700
    color: "#0f1115"

    property alias store: walletStore
    WalletStore { id: walletStore }

    // Approval requests from dApps arrive as provider events; snapshot polling is the fallback.
    Component.onCompleted: {
        if (typeof logos !== "undefined" && logos) {
            logos.onModuleEvent(walletStore.providerModule, "approvalRequested")
            logos.onModuleEvent(walletStore.providerModule, "requestResolved")
        }
    }
    Connections {
        target: typeof logos !== "undefined" ? logos : null
        ignoreUnknownSignals: true
        function onModuleEventReceived(moduleName, eventName, data) {
            if (moduleName === walletStore.providerModule) walletStore.refresh()
        }
    }

    readonly property var pendingTop: walletStore.pendingRequests.length ? walletStore.pendingRequests[0] : null
    readonly property bool unlocked: walletStore.lockState === "unlocked" || walletStore.lockState === "deferred_lock_pending"

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        RowLayout {
            Layout.fillWidth: true
            Label { text: "Lezer Wallet"; color: "#e8eaed"; font.pixelSize: 22; font.bold: true }
            Item { Layout.fillWidth: true }
            Label {
                color: "#9aa0a6"
                text: walletStore.snapshot
                      ? walletStore.snapshot.network_id + (walletStore.snapshot.last_seen_block ? "  ·  block #" + walletStore.snapshot.last_seen_block : "")
                      : (walletStore.providerReachable ? "" : "connecting to lezer_wallet_provider…")
            }
            Button { visible: root.unlocked; text: "Lock"; onClicked: walletStore.call({ op: "lock" }, function () { walletStore.refresh() }) }
        }

        Rectangle {
            Layout.fillWidth: true
            visible: walletStore.lastError.length > 0
            color: "#3b1d1d"; radius: 6; implicitHeight: errLabel.implicitHeight + 16
            Label { id: errLabel; anchors.fill: parent; anchors.margins: 8; text: walletStore.lastError; color: "#f28b82"; wrapMode: Text.Wrap }
        }

        Rectangle {
            Layout.fillWidth: true
            visible: !!walletStore.inFlight
            color: "#1d2a3b"; radius: 6; implicitHeight: 44
            RowLayout {
                anchors.fill: parent; anchors.margins: 10
                BusyIndicator { running: parent.visible; implicitWidth: 24; implicitHeight: 24 }
                Label {
                    color: "#8ab4f8"
                    text: walletStore.inFlight
                          ? walletStore.inFlight.description + (walletStore.inFlight.requires_zk_proof ? " — generating zero-knowledge proof on this device (this can take several minutes)" : " — submitting")
                          : ""
                }
            }
        }

        OnboardingView {
            Layout.fillWidth: true; Layout.fillHeight: true
            visible: !root.unlocked
            store: walletStore
        }

        TabBar {
            id: tabs
            visible: root.unlocked
            Layout.fillWidth: true
            TabButton { text: "Accounts" }
            TabButton { text: "Send" }
            TabButton { text: "Receive" }
            TabButton { text: "Activity" }
            TabButton { text: "Tokens" }
            TabButton { text: "Connected apps" + (walletStore.pendingRequests.length ? " (" + walletStore.pendingRequests.length + ")" : "") }
        }

        StackLayout {
            visible: root.unlocked
            Layout.fillWidth: true; Layout.fillHeight: true
            currentIndex: tabs.currentIndex
            DashboardView { store: walletStore }
            SendView { store: walletStore }
            ReceiveView { store: walletStore }
            ActivityView { store: walletStore }
            TokenView { store: walletStore }
            SessionsView { store: walletStore }
        }
    }

    // Pending dApp requests are shown modally, one at a time, until resolved.
    // Each sheet only ever receives requests of its own kind.
    readonly property var pendingConnect: pendingTop !== null && pendingTop.kind === "connect" ? pendingTop : null
    readonly property var pendingProposal: pendingTop !== null && pendingTop.kind === "proposal" ? pendingTop : null
    ConnectSheet {
        anchors.fill: parent
        visible: root.unlocked && root.pendingConnect !== null
        request: root.pendingConnect
        store: walletStore
    }
    ApprovalSheet {
        anchors.fill: parent
        visible: root.unlocked && root.pendingProposal !== null
        request: root.pendingProposal
        store: walletStore
    }
}
