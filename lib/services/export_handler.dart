import 'dart:io';
import 'package:path_provider/path_provider.dart';

class ExportHandlerService {
  /// Resolves the OS-specific Downloads directory
  static Future<Directory> getDownloadsDirectoryPath() async {
    if (Platform.isWindows) {
      final userProfile = Platform.environment['USERPROFILE'];
      if (userProfile != null) {
        final downloadsPath = Directory('$userProfile\\Downloads');
        if (await downloadsPath.exists()) return downloadsPath;
      }
    }
    final dir = await getDownloadsDirectory() ?? await getApplicationDocumentsDirectory();
    return dir;
  }

  /// Exports text or CSV data cleanly to system Downloads
  static Future<String> exportCsvFile(String fileName, String content) async {
    final downloadsDir = await getDownloadsDirectoryPath();
    final filePath = '${downloadsDir.path}\\$fileName';
    final file = File(filePath);
    await file.writeAsString(content);
    return filePath;
  }
}