package com.maleicacid.tvinput.ipc

import com.maleicacid.tvinput.aribsi.AribSiEngine

object LocalServiceLocator {
    @Volatile var aribSiEngine: AribSiEngine? = null
}
