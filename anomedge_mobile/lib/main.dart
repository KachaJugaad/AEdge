// lib/main.dart
import 'dart:async';
import 'package:flutter/material.dart';
import 'package:connectivity_plus/connectivity_plus.dart';
import 'guidance/guidance_engine.dart';
import 'sync/sync_agent.dart';
import 'tts/tts_service.dart';
import 'screens/alert_screen.dart';
import 'screens/history_screen.dart';
import 'screens/simulator_screen.dart';
import 'screens/settings_screen.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Start core services
  guidanceEngine.start();
  syncAgent.start();
  await ttsService.start(); // TTS needs async init

  runApp(const AnomEdgeApp());
}

class AnomEdgeApp extends StatelessWidget {
  const AnomEdgeApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'AnomEdge',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF1B6CA8),
          brightness: Brightness.dark,
        ),
        useMaterial3: true,
      ),
      home: const AppShell(),
      routes: {
        '/alert': (_) => const AlertScreen(),
        '/history': (_) => const HistoryScreen(),
        '/simulator': (_) => const SimulatorScreen(),
        '/settings': (_) => const SettingsScreen(),
      },
    );
  }
}

class AppShell extends StatefulWidget {
  const AppShell({super.key});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _selectedIndex = 0;
  bool _isOnline = true;
  StreamSubscription<ConnectivityResult>? _connectivitySub;

  final _screens = const [
    AlertScreen(),
    SimulatorScreen(),
    HistoryScreen(),
    SettingsScreen(),
  ];

  @override
  void initState() {
    super.initState();
    _connectivitySub =
        Connectivity().onConnectivityChanged.listen((result) {
      setState(() {
        _isOnline = result != ConnectivityResult.none;
      });
    });
  }

  @override
  void dispose() {
    _connectivitySub?.cancel();
    guidanceEngine.stop();
    syncAgent.stop();
    ttsService.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Column(
        children: [
          if (!_isOnline)
            Container(
              width: double.infinity,
              color: const Color(0xFFF57F17),
              padding:
                  const EdgeInsets.symmetric(vertical: 6, horizontal: 16),
              child: const Row(
                children: [
                  Icon(Icons.wifi_off, color: Colors.white, size: 16),
                  SizedBox(width: 8),
                  Text(
                    'OFFLINE — Events queued for sync',
                    style: TextStyle(
                      color: Colors.white,
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                      letterSpacing: 0.5,
                    ),
                  ),
                ],
              ),
            ),
          // IndexedStack keeps ALL screens alive — no state loss on tab switch
          Expanded(
            child: IndexedStack(
              index: _selectedIndex,
              children: _screens,
            ),
          ),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        backgroundColor: const Color(0xFF161B22),
        selectedIndex: _selectedIndex,
        onDestinationSelected: (i) =>
            setState(() => _selectedIndex = i),
        destinations: const [
          NavigationDestination(
            icon: Icon(Icons.notifications_outlined),
            selectedIcon: Icon(Icons.notifications),
            label: 'Alerts',
          ),
          NavigationDestination(
            icon: Icon(Icons.play_circle_outline),
            selectedIcon: Icon(Icons.play_circle),
            label: 'Simulator',
          ),
          NavigationDestination(
            icon: Icon(Icons.history_outlined),
            selectedIcon: Icon(Icons.history),
            label: 'History',
          ),
          NavigationDestination(
            icon: Icon(Icons.settings_outlined),
            selectedIcon: Icon(Icons.settings),
            label: 'Settings',
          ),
        ],
      ),
    );
  }
}
