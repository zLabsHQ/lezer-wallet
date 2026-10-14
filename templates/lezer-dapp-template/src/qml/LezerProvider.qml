import QtQuick

QtObject {
    id: provider
    property string moduleName: "lezer_wallet_provider"
    property string sessionToken: ""

    function connectDapp(metadataJson) {}
    function proposeTransaction(intentJson) {}
}
