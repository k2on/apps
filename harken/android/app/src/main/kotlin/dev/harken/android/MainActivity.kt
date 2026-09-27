package dev.harken.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.viewmodel.compose.viewModel

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val gold = Color(0xFFC9A227)
            val scheme = if (isSystemInDarkTheme()) {
                darkColorScheme(primary = gold, onPrimary = Color(0xFF1E1E1E))
            } else {
                lightColorScheme(primary = Color(0xFF8A6D0B), onPrimary = Color.White)
            }
            MaterialTheme(colorScheme = scheme) {
                val model: Model = viewModel()
                HarkenApp(model)
            }
        }
    }
}
