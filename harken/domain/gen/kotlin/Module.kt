package harken.gen

import dev.arkdb.authoring.*
import dev.arkdb.authoring.Int
import dev.arkdb.authoring.List

fun module(): Module = Module(library(), playlists())
