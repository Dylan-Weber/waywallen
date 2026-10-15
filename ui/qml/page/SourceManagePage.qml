pragma ComponentBehavior: Bound
pragma ValueTypeBehavior: Assertable
import QtQuick
import Qcm.Material as MD
import waywallen.ui as W

MD.Page {
    id: root
    title: qsTr("Library Manager")
    // Bind locally; Qt.binding values in asynchronous initial properties can crash Qt.
    property QtObject visibilityState: null
    readonly property W.WallpaperListQuery libraryQuery: visibilityState ? visibilityState.query : null
    readonly property bool visibilityReady: visibilityState !== null && visibilityState.ready
    readonly property var hiddenIds: libraryQuery ? Array.from(libraryQuery.hiddenLibraryIds) : []
    readonly property var missingIds: {
        const libraries = Array.from(W.App.libraryManager.libraries);
        if (!libraryQuery || !W.App.libraryManager.initialized)
            return [];
        return hiddenIds.filter(id => !libraries.some(library => library.id === id));
    }

    actions: [
        MD.Action {
            icon.name: MD.Token.icon.add
            text: qsTr("Add library")
            onTriggered: root.MD.MProp.page.pushItem('waywallen.ui/AddLibraryPage')
        }
    ]

    W.LibraryRemoveQuery {
        id: removeQuery
    }

    contentItem: Item {
        implicitHeight: Math.max(m_view.contentHeight, emptyLabel.implicitHeight) + 16
        implicitWidth: m_view.implicitWidth

        MD.VerticalListView {
            id: m_view
            width: parent.width
            height: parent.height
            model: W.App.libraryManager.libraries
            spacing: 8

            topMargin: 8
            bottomMargin: 8
            leftMargin: 12
            rightMargin: 12

            footer: Column {
                width: m_view.contentWidth
                spacing: 8
                Repeater {
                    model: root.missingIds
                    delegate: MD.ListItem {
                        id: missingSourceItem
                        required property var modelData
                        width: m_view.contentWidth
                        text: qsTr("Removed source #%1").arg(modelData)
                        trailing: MD.IconButton {
                            enabled: root.visibilityReady
                            icon.name: MD.Token.icon.close
                            Accessible.name: qsTr("Clear hidden state")
                            MD.ToolTip.text: Accessible.name
                            MD.ToolTip.visible: hovered
                            onClicked: root.libraryQuery.toggleLibraryHidden(missingSourceItem.modelData)
                        }
                    }
                }
            }

            delegate: MD.ListItem {
                id: sourceItem
                required property var modelData
                readonly property string fullPath: String(modelData.path || "")

                width: m_view.contentWidth
                radius: 8

                mdState.backgroundColor: MD.Token.color.surface_container

                text: fullPath
                wrapMode: Text.Wrap
                maximumLineCount: 3
                supportText: qsTr("Plugin: %1").arg(modelData.pluginName)

                leader: MD.Icon {
                    name: MD.Token.icon.folder
                    color: MD.Token.color.on_surface_variant
                }

                trailing: Row {
                    spacing: 4
                    MD.IconButton {
                        visible: root.libraryQuery !== null
                        enabled: root.visibilityReady
                        checked: !root.hiddenIds.includes(sourceItem.modelData.id)
                        icon.name: checked ? MD.Token.icon.visibility : MD.Token.icon.visibility_off
                        Accessible.name: checked ? qsTr("Hide source") : qsTr("Show source")
                        MD.ToolTip.text: Accessible.name
                        MD.ToolTip.visible: hovered
                        onClicked: root.libraryQuery.toggleLibraryHidden(sourceItem.modelData.id)
                    }
                    MD.IconButton {
                        icon.name: MD.Token.icon.delete
                        Accessible.name: qsTr("Remove library")
                        MD.ToolTip.text: Accessible.name
                        MD.ToolTip.visible: hovered
                        onClicked: {
                            removeQuery.libraryId = sourceItem.modelData.id;
                            removeQuery.reload();
                        }
                    }
                }
            }
        }

        MD.Text {
            id: emptyLabel
            visible: W.App.libraryManager.initialized && W.App.libraryManager.count === 0 && root.missingIds.length === 0
            anchors.centerIn: parent
            width: Math.max(0, parent.width - 24)
            text: qsTr("No libraries")
            typescale: MD.Token.typescale.body_medium
            color: MD.Token.color.on_surface_variant
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
        }
    }
}
