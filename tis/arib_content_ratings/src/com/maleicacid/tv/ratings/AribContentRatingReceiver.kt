package com.maleicacid.tv.ratings

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** PackageManager discovers the rating-system XML from manifest metadata. */
class AribContentRatingReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) = Unit
}
