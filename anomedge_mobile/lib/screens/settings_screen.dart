// lib/screens/settings_screen.dart
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../tts/tts_service.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  bool _ttsEnabled = true;
  bool _vibrationEnabled = true;
  String _driverMode = 'Classic'; // Classic or Assist (Phase 1)
  String _assetId = 'ASSET-001';
  String _driverId = 'DRIVER-001';

  @override
  void initState() {
    super.initState();
    _loadSettings();
  }

  Future<void> _loadSettings() async {
    final prefs = await SharedPreferences.getInstance();
    setState(() {
      _ttsEnabled = prefs.getBool('tts_enabled') ?? true;
      _vibrationEnabled = prefs.getBool('vibration_enabled') ?? true;
      _driverMode = prefs.getString('driver_mode') ?? 'Classic';
      _assetId = prefs.getString('asset_id') ?? 'ASSET-001';
      _driverId = prefs.getString('driver_id') ?? 'DRIVER-001';
    });
    ttsService.setEnabled(_ttsEnabled);
  }

  Future<void> _saveSetting(String key, dynamic value) async {
    final prefs = await SharedPreferences.getInstance();
    if (value is bool) await prefs.setBool(key, value);
    if (value is String) await prefs.setString(key, value);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D1117),
      appBar: AppBar(
        backgroundColor: const Color(0xFF161B22),
        title: const Text('Settings',
            style: TextStyle(color: Colors.white)),
        iconTheme: const IconThemeData(color: Colors.white),
      ),
      body: ListView(
        children: [
          _sectionHeader('ALERTS'),
          _toggleTile(
            'Text-to-Speech',
            'Speak HIGH and CRITICAL alerts aloud',
            Icons.volume_up_outlined,
            _ttsEnabled,
            (v) {
              setState(() => _ttsEnabled = v);
              _saveSetting('tts_enabled', v);
              ttsService.setEnabled(v); // actually enable/disable the service
            },
          ),
          _toggleTile(
            'Vibration',
            'Vibrate on HIGH and CRITICAL alerts',
            Icons.vibration,
            _vibrationEnabled,
            (v) {
              setState(() => _vibrationEnabled = v);
              _saveSetting('vibration_enabled', v);
            },
          ),
          _sectionHeader('DRIVER MODE'),
          _radioTile('Classic', 'Rule-based guidance only'),
          _radioTile('Assist', 'AI-enhanced guidance (Phase 2)'),
          _sectionHeader('DEVICE'),
          _infoTile('Asset ID', _assetId, Icons.directions_car_outlined),
          _infoTile('Driver ID', _driverId, Icons.person_outline),
          _sectionHeader('ABOUT'),
          _infoTile('Version', '1.0.0 — Phase 0', Icons.info_outline),
          _infoTile('Build', 'AnomEdge Buildathon 2026', Icons.build_outlined),
        ],
      ),
    );
  }

  Widget _sectionHeader(String title) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 24, 16, 8),
        child: Text(title,
            style: const TextStyle(
                color: Colors.white38,
                fontSize: 11,
                letterSpacing: 1.5)),
      );

  Widget _toggleTile(String title, String subtitle, IconData icon,
      bool value, ValueChanged<bool> onChanged) {
    return ListTile(
      tileColor: const Color(0xFF161B22),
      leading: Icon(icon, color: Colors.white54),
      title:
          Text(title, style: const TextStyle(color: Colors.white)),
      subtitle:
          Text(subtitle, style: const TextStyle(color: Colors.white38)),
      trailing: Switch(
        value: value,
        onChanged: onChanged,
        thumbColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected)
              ? const Color(0xFF1B6CA8)
              : null,
        ),
      ),
    );
  }

  Widget _radioTile(String mode, String description) {
    final selected = _driverMode == mode;
    return ListTile(
      tileColor: const Color(0xFF161B22),
      title: Text(mode, style: const TextStyle(color: Colors.white)),
      subtitle: Text(description,
          style: const TextStyle(color: Colors.white38)),
      trailing: selected
          ? const Icon(Icons.check_circle, color: Color(0xFF1B6CA8))
          : const Icon(Icons.circle_outlined, color: Colors.white38),
      onTap: () {
        setState(() => _driverMode = mode);
        _saveSetting('driver_mode', mode);
      },
    );
  }

  Widget _infoTile(String title, String value, IconData icon) {
    return ListTile(
      tileColor: const Color(0xFF161B22),
      leading: Icon(icon, color: Colors.white54),
      title:
          Text(title, style: const TextStyle(color: Colors.white)),
      trailing: Text(value,
          style: const TextStyle(color: Colors.white54, fontSize: 13)),
    );
  }
}
