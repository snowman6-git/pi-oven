package dev.code4hub.pioven

import android.content.res.Configuration
import android.graphics.drawable.ColorDrawable
import android.os.Bundle
import android.view.View
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

// No edge-to-edge: the page sits between the status bar and the navigation bar, and the strips above and below it
// are painted in the app's own background colour (colors.xml: dark by default, the light paper colour in day mode).
class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val bg = ContextCompat.getColor(this, R.color.app_bg)
    WindowCompat.setDecorFitsSystemWindows(window, true)
    window.setBackgroundDrawable(ColorDrawable(bg))
    @Suppress("DEPRECATION")
    window.statusBarColor = bg
    @Suppress("DEPRECATION")
    window.navigationBarColor = bg
    val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
    val root = findViewById<View>(android.R.id.content)
    WindowInsetsControllerCompat(window, root).apply {
      isAppearanceLightStatusBars = !night
      isAppearanceLightNavigationBars = !night
    }
    // Newer Android versions draw behind the bars whatever is asked for, so the bars' room is given back as padding.
    ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
      val b = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime())
      v.setPadding(b.left, b.top, b.right, b.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(root)
  }
}
