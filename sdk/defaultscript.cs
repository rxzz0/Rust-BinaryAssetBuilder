//===================================================//
//===== ABOUT =======================================//
//===================================================//
//                                                   //
// Extended build script for the Red Alert 3 Mod SDK //
//                                                   //
// Author: Bibber                                    //
// eMail: m.bibber@live.com                           //
// Homepage: http://bibber.bplaced.net               //
//                                                   //
//===================================================//
//===== PROGRAM =====================================//
//===================================================//

using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using Microsoft.Win32;

namespace EALABuild
{
    class BuildScript: EALABuildScript
    {
		#region Member variables
        Dictionary<string, Object> currentGUIData;
        static int MOD_BUILD_STEPS = 11;
        static string DEFAULT_GAME_VERSION = "1.12";
		static string DEFAULT_MOD_VERSION = "1.00";

		// Variables
		object Reg;
		string Cmd = Environment.GetEnvironmentVariable("comspec");
		string PersonalDirectory;
		string SDKDirectory;
		string UserDataLeafName;

		string RA3Dir;
		string RA3DataDir;
		string BuiltModsPath;

		string Mod;
		string ModPath;
		string ModAdditionalFilesPath;
		string ModAssetsPath;
		string ModDataPath;
		string BuiltModPath;
		string BuiltModDataPath;
		string ModInstallPath;

		string ModBabProj;
		string ModXml;
		string ModBase;
		string ModBigMisc;
		string ModBigStreams;
		string ModSkudef;
		#endregion

		#region Implementation of EALABuildScript interface.
        public static void CopyFolders(string source, string destination)
        {
			if (!Directory.Exists(destination)) Directory.CreateDirectory(destination);

            DirectoryInfo di = new DirectoryInfo(source);
            CopyFiles(source, destination);

            foreach (DirectoryInfo d in di.GetDirectories())
            {
                string newDir = Path.Combine(destination, d.Name);
                CopyFolders(d.FullName, newDir);
            }
        }

        public static void CopyFiles(string source, string destination)
        {
            DirectoryInfo di = new DirectoryInfo(source);
            FileInfo[] files = di.GetFiles();

            foreach (FileInfo f in files)
            {
                string sourceFile = f.FullName;
                string destFile = Path.Combine(destination, f.Name);
                File.Copy(sourceFile, destFile, true);
            }
        }

		// This function is called immediately after the script constructor in the GUI load process.
        public void initialize()
        {
			/*
			* This will automatically reset the counter in BuildHelper, and the GUI will
			* set the type of build in BuildHelper.  If it is not enabled, call
			* BuildHelper.Reset, and set BuildHelper.CurrentBuildType = to either BuildType.ModBuild
			* or BuildType.MapBuild in the buildMap and buildMod functions
			*/

            BuildHelper.AutomaticallyResetBuildHelper = true;
            BuildHelper.EnableBenchmarking = true;

			// Personal Directory
			PersonalDirectory = Environment.ExpandEnvironmentVariables(Registry.GetValue("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders", "Personal", "").ToString());

			// The running Studio belongs to this SDK, even if an older copy is registered.
			string studioPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "EALAModStudio.exe");
			if (File.Exists(studioPath)) {
				SDKDirectory = Path.GetDirectoryName(studioPath);
			} else {
				const string uninstallKey = "\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{F6A3F605-7B10-4939-8D3D-4594332C1649}";
				object installed = Registry.GetValue("HKEY_LOCAL_MACHINE\\Software" + uninstallKey, "InstallLocation", null)
					?? Registry.GetValue("HKEY_LOCAL_MACHINE\\Software\\Wow6432Node" + uninstallKey, "InstallLocation", null);
				string registeredPath = installed as string;
				SDKDirectory = !String.IsNullOrEmpty(registeredPath) && Directory.Exists(registeredPath)
					? Path.GetDirectoryName(Path.Combine(Path.GetFullPath(registeredPath), "EALAModStudio.exe"))
					: StaticPaths.CurrentDirectory;
			}

			// Game Install Dir
			Reg = Registry.GetValue("HKEY_LOCAL_MACHINE\\Software\\Electronic Arts\\Electronic Arts\\Red Alert 3", "Install Dir", null);
			if (Reg == null) {
				Reg = Registry.GetValue("HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Electronic Arts\\Electronic Arts\\Red Alert 3", "Install Dir", null);
				if (Reg == null) {
					RA3Dir = StaticPaths.CurrentDirectory;
				} else {
					RA3Dir = Reg.ToString();
				}
			} else {
				RA3Dir = Reg.ToString();
			}

			// User Data Leaf Name
			Reg = Registry.GetValue("HKEY_LOCAL_MACHINE\\Software\\Electronic Arts\\Electronic Arts\\Red Alert 3", "UserDataLeafName", null);
			if (Reg == null) {
				Reg = Registry.GetValue("HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Electronic Arts\\Electronic Arts\\Red Alert 3", "UserDataLeafName", null);
				if (Reg == null) {
					UserDataLeafName = "Red Alert 3";
				} else {
					UserDataLeafName = Reg.ToString();
				}
			} else {
				UserDataLeafName = Reg.ToString();
			}

			// Variables
			RA3DataDir = RA3Dir + "\\data";
			BuiltModsPath = SDKDirectory + "\\builtmods";
        }

		// This function is called on the successful completion of a build step.
        public void onStepSuccess(int stepId)
        {
            if (stepId == 9 && !UseEaCompiler() && shouldExecuteStep(10) && OverlapMiscBigs())
                StartMiscBig();
            if (BuildHelper.GetNextExecutableStep() != -1)
            {
                executeModBuildStep(BuildHelper.CurrentStep);
            }
            else
            {
                BuildHelper.OnBuildComplete();
            }
        }

		// This function is called on the failure of a build step.
        public void onStepFailure(int stepId)
        {
            StopBigBuild();
            BuildHelper.DisplayLine(String.Format("Build failed on step {0}", stepId));
        }

		// This function is called to determine if a step should execute.
        public bool shouldExecuteStep(int stepId)
        {
            if (stepId == 9)
            {
                object compileMaps;
                return currentGUIData.TryGetValue("compileMissingMapOverrides", out compileMaps)
                    && compileMaps is bool && (bool)compileMaps;
            }
            if (currentGUIData.ContainsKey("step" + stepId))
            {
                if ((bool)currentGUIData["step" + stepId])
                {
                    return true;
                }
                else
                {
                    return false;
                }
            }
            return true;
        }

		// This function is called when the user elects to build a mod
        public void buildMod(string modName, Dictionary<string, Object> GUIResults)
        {
            currentGUIData = GUIResults;
            BuildHelper.CurrentBuildSteps = MOD_BUILD_STEPS;

			//Variables
			Mod = BuildHelper.BuildTarget;
			ModPath = SDKDirectory + "\\mods\\" + Mod;
			ModAdditionalFilesPath = ModPath + "\\misc";
			ModAssetsPath = ModPath + "\\assets";
			ModDataPath = ModPath + "\\data";
			BuiltModPath = BuiltModsPath + "\\mods\\" + Mod;
			BuiltModDataPath = BuiltModPath + "\\data";
			ModInstallPath = PersonalDirectory + "\\" + UserDataLeafName + "\\Mods\\" + Mod;

			ModBabProj = ModPath + "\\mod.babproj";
			ModXml = ModDataPath + "\\mod.xml";
			ModBase = Mod + "_" + (string)currentGUIData["modversion"];
			ModBigMisc = ModBase + "_Misc.big";
			ModBigStreams = ModBase + "_Streams.big";
			ModSkudef = ModBase + ".skudef";

            StopBigBuild();
            if (!UseEaCompiler() && shouldExecuteStep(10) && !shouldExecuteStep(9) && OverlapMiscBigs())
                StartMiscBig();
            executeModBuildStep(BuildHelper.GetNextExecutableStep());
        }

		/*
		* This function is called during the GUI initialization to generate
		* the GUI for the mod build options section.
		*/
        public List<GUIElementData> GetModBuildGUIData()
        {
            List<GUIElementData> modGUIData = new List<GUIElementData>();
            modGUIData.Add(new GUIElementData("useEaCompiler", "Use original EA compiler", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step1", "Clear Build Cache", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step2", "Clear Session Cache", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step3", "Clear Built Mod", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("sp1", "", GUIElementData.LabelType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step4", "Build AptUI", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("step5", "Build Global Data", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("step6", "Build Static Data", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step7", "Merge Assets", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("step8", "Fix Assets", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("compileMissingMapOverrides", "Compile missing map overrides", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("sp2", "", GUIElementData.LabelType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step10", "Create BIG Files", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
			modGUIData.Add(new GUIElementData("step11", "Create Skudef File", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("showDetailedBuildlog", "Show detailed buildlog", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("hideRustWarnings", "Hide Rust warnings", GUIElementData.CheckBoxType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("mv", "Mod Version:", GUIElementData.LabelType, ScriptIndex.ModScript));
            modGUIData.Add(new GUIElementData("modversion", DEFAULT_MOD_VERSION, GUIElementData.InputBoxType, ScriptIndex.ModScript));
			return modGUIData;
        }
        #endregion


        private string bigStage;
        private System.Diagnostics.Process miscBig;
        private StringBuilder miscBigLog;

        private System.Diagnostics.Process StartBig(string input, string name, out StringBuilder output)
        {
            if (bigStage == null)
            {
                string stage = Path.Combine(ModInstallPath, ".rust-big-" + Guid.NewGuid().ToString("N"));
                Directory.CreateDirectory(stage);
                bigStage = stage;
            }
            System.Diagnostics.ProcessStartInfo start = new System.Diagnostics.ProcessStartInfo(
                Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), String.Format("pack-big \"{0}\" --output \"{1}\" --reuse \"{2}\"",
                    input, Path.Combine(bigStage, name), Path.Combine(ModInstallPath, name)));
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.RedirectStandardError = true;
            bool detailed = ShowDetailedBuildlog();
            start.EnvironmentVariables["BAB_DETAILED_BUILDLOG"] = detailed ? "1" : "0";
            start.RedirectStandardOutput = detailed;
            StringBuilder log = detailed ? new StringBuilder() : null;
            output = log;
            System.Diagnostics.Process process = System.Diagnostics.Process.Start(start);
            if (detailed)
            {
                process.OutputDataReceived += delegate(object sender, System.Diagnostics.DataReceivedEventArgs line)
                {
                    if (line.Data != null)
                        log.AppendLine(line.Data);
                };
                process.BeginOutputReadLine();
            }
            return process;
        }

        private void WaitForBig(System.Diagnostics.Process process, StringBuilder log)
        {
            string error = process.StandardError.ReadToEnd();
            process.WaitForExit();
            if (log != null)
            {
                using (StringReader lines = new StringReader(log.ToString()))
                    for (string line = lines.ReadLine(); line != null; line = lines.ReadLine())
                        BuildHelper.DisplayLine(line);
            }
            if (process.ExitCode != 0)
                throw new IOException(error.Trim());
        }

        private void StopBigBuild()
        {
            try
            {
                if (miscBig != null)
                {
                    using (miscBig)
                    {
                        if (!miscBig.HasExited)
                        {
                            try { miscBig.Kill(); }
                            catch (InvalidOperationException)
                            {
                                if (!miscBig.HasExited) throw;
                            }
                        }
                        miscBig.WaitForExit();
                    }
                }
                if (bigStage != null)
                {
                    foreach (string path in Directory.GetFiles(bigStage))
                        File.Delete(path);
                    Directory.Delete(bigStage);
                }
            }
            catch (Exception error)
            {
                BuildHelper.DisplayLine("Warning: BIG build cleanup failed. " + error.Message);
            }
            finally
            {
                miscBig = null;
                miscBigLog = null;
                bigStage = null;
            }
        }

        private static bool OverlapMiscBigs()
        {
            return Environment.GetEnvironmentVariable("BAB_OVERLAP_MISC") == "1";
        }

        private void StartMiscBig()
        {
            if (!Directory.Exists(ModAdditionalFilesPath))
                return;
            try
            {
                miscBig = StartBig(ModAdditionalFilesPath, ModBigMisc, out miscBigLog);
            }
            catch (Exception error)
            {
                BuildHelper.DisplayLine("Warning: Could not start Misc BIG early. " + error.Message);
                StopBigBuild();
            }
        }

        private void PublishBigs()
        {
            List<string> moved = new List<string>();
            try
            {
                foreach (string name in new string[] { ModBigStreams, ModBigMisc })
                {
                    string staged = Path.Combine(bigStage, name);
                    if (!File.Exists(staged))
                        continue;
                    string target = Path.Combine(ModInstallPath, name);
                    if (File.Exists(target))
                        File.Replace(staged, target, staged + ".previous");
                    else
                        File.Move(staged, target);
                    moved.Add(name);
                }
            }
            catch
            {
                try
                {
                    for (int i = moved.Count - 1; i >= 0; --i)
                    {
                        string name = moved[i];
                        string target = Path.Combine(ModInstallPath, name);
                        string backup = Path.Combine(bigStage, name) + ".previous";
                        if (File.Exists(backup))
                            File.Replace(backup, target, null);
                        else
                            File.Delete(target);
                    }
                }
                catch (Exception error)
                {
                    string recovery = bigStage;
                    bigStage = null;
                    throw new IOException("BIG rollback failed. Recovery files are in " + recovery, error);
                }
                throw;
            }
        }

        private void FinishBigBuild()
        {
            bool success = false;
            try
            {
                if (Directory.Exists(BuiltModPath))
                {
                    StringBuilder streamsLog;
                    using (System.Diagnostics.Process streams = StartBig(BuiltModPath, ModBigStreams, out streamsLog))
                        WaitForBig(streams, streamsLog);
                }
                if (miscBig == null && Directory.Exists(ModAdditionalFilesPath))
                    miscBig = StartBig(ModAdditionalFilesPath, ModBigMisc, out miscBigLog);
                if (miscBig != null)
                    WaitForBig(miscBig, miscBigLog);
                if (bigStage != null)
                    PublishBigs();
                success = true;
            }
            catch (Exception error)
            {
                BuildHelper.DisplayLine("Critical: Creating BIG files failed. " + error.Message);
            }
            finally
            {
                StopBigBuild();
            }
            RunExecutableArguments result = new RunExecutableArguments(Cmd);
            result.Arguments = success ? "/C exit /b 0" : "/C echo Critical: Creating BIG files failed. & exit /b 1";
            BuildHelper.RunStep(StepType.RunExecutable, result);
        }

        private bool ShowDetailedBuildlog()
        {
            object value;
            return currentGUIData.TryGetValue("showDetailedBuildlog", out value) && value is bool && (bool)value;
        }

        private void RunRustBuildStep(RunExecutableArguments args)
        {
            args.Arguments = args.Arguments.Insert(3, ShowDetailedBuildlog()
                ? "(set \"BAB_DETAILED_BUILDLOG=1\") & " : "(set \"BAB_DETAILED_BUILDLOG=0\") & ");
            args.Arguments = args.Arguments.Insert(3, "(if not defined BAB_ASSET_WORKERS set \"BAB_ASSET_WORKERS=4\") & ");
            object hideWarnings;
            if (currentGUIData.TryGetValue("hideRustWarnings", out hideWarnings) && hideWarnings is bool && (bool)hideWarnings)
            {
                args.Arguments = args.Arguments.Insert(3, "(set \"BAB_NO_WARNINGS=1\") & ");
            }
            args.Arguments = "/D /V:OFF " + args.Arguments;
            BuildHelper.RunStep(StepType.RunExecutable, args);
        }

        private static string StaticBuildPathError(string compiler, string modXml)
        {
            if (!File.Exists(compiler))
                return "Rust compiler missing at '" + compiler + "'. Run Install-RustCompiler.cmd and select this SDK folder.";
            if (!File.Exists(modXml))
                return "Mod XML missing at '" + modXml + "'. Check the selected mod's Data folder.";
            return null;
        }

        private bool UseEaCompiler()
        {
            object setting;
            return currentGUIData.TryGetValue("useEaCompiler", out setting) && setting is bool && (bool)setting;
        }

        private void ExecuteEaBuildStep(int stepId)
        {
            switch (stepId)
            {
                case 4:
					BuildHelper.DisplayLine(String.Format("Step {0}: Building AptUI",
						stepId));
					RunExecutableArguments argsAptUI = new RunExecutableArguments(Cmd);
					argsAptUI.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (for %I in (\"{1}\\aptui\\*\") do (del \"%I\" /F /Q))"
						+ " & (for %I in (\"{3}\\aptui\\*.xml\") do ("
							+ "(\"{2}\" \"%I\" /od:\"{4}\" /iod:\"{4}\" /csc:false /ls:true /osh:false /pc:true /res:true /slowclean:true /ss:true /art:\".;.\\Mods\\{5}\\Art;.\\Mods;.\\Art\" /audio:\".;.\\Mods\\{5}\\Audio;.\\Mods;.\\Audio\" /data:\".;.\\Mods\\{5}\\Data;.\\Mods;.\\SageXml\")"
							+ " & (if exist \"{1}\\aptui\\%~nI.manifest\" (echo. >\"{1}\\aptui\\%~nI.version\"))"
						+ "))",
						SDKDirectory, BuiltModDataPath, Path.Combine(SDKDirectory, @"tools\binaryassetbuilder.exe"), ModDataPath, BuiltModsPath, Mod);
					BuildHelper.RunStep(StepType.RunExecutable, argsAptUI);
					break;
                case 5:
					BuildHelper.DisplayLine(String.Format("Step {0}: Building global data",
						stepId));
					RunExecutableArguments argsGlobal = new RunExecutableArguments(Cmd);
					argsGlobal.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (for %I in (\"{1}\\additionalmaps\\mapmetadata_*\") do (del \"%I\" /F /Q))"
						+ " & (for %I in (\"{3}\\additionalmaps\\mapmetadata_*.xml\") do ("
							+ "(\"{2}\" \"%I\" /od:\"{4}\" /iod:\"{4}\" /csc:false /ls:true /osh:false /pc:true /res:true /slowclean:true /ss:true /art:\".;.\\Mods\\{5}\\Art;.\\Mods;.\\Art\" /audio:\".;.\\Mods\\{5}\\Audio;.\\Mods;.\\Audio\" /data:\".;.\\Mods\\{5}\\Data;.\\Mods;.\\SageXml\")"
							+ " & (if exist \"{1}\\additionalmaps\\%~nI.manifest\" (echo. >\"{1}\\additionalmaps\\%~nI.version\"))"
						+ "))",
						SDKDirectory, BuiltModDataPath, Path.Combine(SDKDirectory, @"tools\binaryassetbuilder.exe"), ModDataPath, BuiltModsPath, Mod);
					BuildHelper.RunStep(StepType.RunExecutable, argsGlobal);
					break;
                case 6:
					BuildHelper.DisplayLine(String.Format("Step {0}: Building static data",
						stepId));
                    RunExecutableArguments args = new RunExecutableArguments(Cmd);
					args.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{1}\\mod.bin\" (del \"{1}\\mod.bin\" /F /Q))"
						+ " & (if exist \"{1}\\mod.imp\" (del \"{1}\\mod.imp\" /F /Q))"
						+ " & (if exist \"{1}\\mod.manifest\" (del \"{1}\\mod.manifest\" /F /Q))"
						+ " & (if exist \"{1}\\mod.relo\" (del \"{1}\\mod.relo\" /F /Q))"
						+ " & (if exist \"{1}\\mod.version\" (del \"{1}\\mod.version\" /F /Q))"
						+ " & (for %I in (\"{1}\\mod_*\") do (del \"%I\" /F /Q))"
						+ " & (\"{2}\" \"{3}\" /od:\"{4}\" /iod:\"{4}\" /csc:false /ls:true /osh:false /pc:true /res:true /slowclean:true /ss:true /art:\".;.\\Mods\\{5}\\Art;.\\Mods;.\\Art\" /audio:\".;.\\Mods\\{5}\\Audio;.\\Mods;.\\Audio\" /data:\".;.\\Mods\\{5}\\Data;.\\Mods;.\\SageXml\")"
						+ " & (if exist \"{1}\\mod.manifest\" (echo.>>\"{1}\\mod.version\"))",
						SDKDirectory, BuiltModDataPath, Path.Combine(SDKDirectory, @"tools\binaryassetbuilder.exe"), ModXml, BuiltModsPath, Mod);

					if (((bool)currentGUIData["step7"] == false) && ((bool)currentGUIData["step8"] == false)) {
						args.Arguments = args.Arguments + String.Format(" & (\"{0}\" \"{1}\\mod.manifest\" /f)",
							Path.Combine(SDKDirectory, @"tools\lodstreambuilder.exe"), BuiltModDataPath);
					}

					BuildHelper.RunStep(StepType.RunExecutable, args);
                    break;
                case 7:
					BuildHelper.DisplayLine(String.Format("Step {0}: Merging assets",
						stepId));
					RunExecutableArguments argsMerge = new RunExecutableArguments(Cmd);
					argsMerge.Arguments = String.Format("/V:ON /C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{2}\\mod.manifest\" (if exist \"{3}\\mod\" ("
							+ "(\"{1}\" \"{2}\\mod.manifest\" \"{3}\\mod\")"
						+ ")))"
						+ " & (for %I in (\"{2}\\additionalmaps\\mapmetadata_*.manifest\") do ("
							+ "(if exist \"{3}\\additionalmaps\\%~nI\" ("
								+ "(\"{1}\" \"%I\" \"{3}\\additionalmaps\\%~nI\")"
								+ " & (\"{4}\" \"%I\")"
							+ "))"
						+ "))"
						+ " & (for %I in (\"{2}\\aptui\\*.manifest\") do ("
							+ "(if exist \"{3}\\aptui\\%~nI\" ("
								+ "(\"{1}\" \"%I\" \"{3}\\aptui\\%~nI\")"
								+ " & (\"{4}\" \"%I\")"
							+ "))"
						+ "))"
						+ " & (cd /D \"{0}\")",
						SDKDirectory, Path.Combine(SDKDirectory, @"tools\assetmerger.exe"), BuiltModDataPath, ModAssetsPath, Path.Combine(SDKDirectory, @"tools\streamsorter.exe"));

					if ((bool)currentGUIData["step8"] == false) {
						argsMerge.Arguments = argsMerge.Arguments + String.Format(" & (if exist \"{1}\\mod.manifest\" ("
							+ "(\"{2}\" \"{1}\\mod.manifest\")"
							+ " & (\"{0}\" \"{1}\\mod.manifest\" /f)"
						+ "))",
							Path.Combine(SDKDirectory, @"tools\lodstreambuilder.exe"), BuiltModDataPath, Path.Combine(SDKDirectory, @"tools\streamsorter.exe"));
					}

					BuildHelper.RunStep(StepType.RunExecutable, argsMerge);
					break;
                case 8:
					BuildHelper.DisplayLine(String.Format("Step {0}: Fixing assets",
						stepId));
					RunExecutableArguments argsFix = new RunExecutableArguments(Cmd);
					argsFix.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (\"{2}\" \"{1}\\mod.manifest\" \"{6}\\sagexml\\worldbuilder.manifest\")"
						+ " & (\"{3}\" \"{1}\\mod.manifest\" \"{7}\\wbdata_12.big\" \"{6}\\audio.manifest\" \"{6}\\global.manifest\" \"{6}\\static.manifest\" \"{6}\\stringhashes.xml\")"
						+ " & (\"{4}\" \"{1}\\mod.manifest\")"
						+ " & (\"{5}\" \"{1}\\mod.manifest\" /f)",
						SDKDirectory, BuiltModDataPath, Path.Combine(SDKDirectory, @"tools\hashfix.exe"), Path.Combine(SDKDirectory, @"tools\modassetresolver.exe"), Path.Combine(SDKDirectory, @"tools\streamsorter.exe"), Path.Combine(SDKDirectory, @"tools\lodstreambuilder.exe"), BuiltModsPath, RA3DataDir);
					BuildHelper.RunStep(StepType.RunExecutable, argsFix);
					break;
                case 10:
					BuildHelper.DisplayLine(String.Format("Step {0}: Creating big files",
							stepId));
					RunExecutableArguments argsBig = new RunExecutableArguments(Cmd);
					argsBig.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{2}\" (\"{1}\" -f \"{2}\" -x:*.asset -o:\"{3}\\{4}\"))"
						+ " & (if exist \"{6}\" (\"{1}\" -f \"{6}\" -x:*.asset -o:\"{3}\\{5}\"))",
						SDKDirectory, Path.Combine(SDKDirectory, @"tools\makebig.exe"), BuiltModPath, ModInstallPath, ModBigStreams, ModBigMisc, ModAdditionalFilesPath);
                    BuildHelper.RunStep(StepType.RunExecutable, argsBig);
                    break;
            }
        }

        private void executeModBuildStep(int stepId)
        {
            if (stepId >= 4 && stepId <= 10 && stepId != 9 && UseEaCompiler())
            {
                ExecuteEaBuildStep(stepId);
                return;
            }
            switch(stepId)
            {
                case 1:
					BuildHelper.DisplayLine(String.Format("Step {0}: Clearing build cache",
							stepId));
                    RunExecutableArguments clearBuildCache = new RunExecutableArguments(Cmd);
                    clearBuildCache.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{1}\\cache\" (rd \"{1}\\cache\" /S /Q))",
						SDKDirectory, BuiltModsPath);
					BuildHelper.RunStep(StepType.RunExecutable, clearBuildCache);
                    break;
                case 2:
					BuildHelper.DisplayLine(String.Format("Step {0}: Clearing session cache",
							stepId));
                    RunExecutableArguments clearSessionCache = new RunExecutableArguments(Cmd);
                    clearSessionCache.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{1}\\builtmods\" (rd \"{1}\\builtmods\" /S /Q))"
						+ " & (for /R \"{2}\" %I in (\"*.asset\") do (del \"%I\" /F /Q))"
						+ " & (if exist \"{1}\\binaryassetbuilder.sessioncache.xml\" (del \"{1}\\binaryassetbuilder.sessioncache.xml\" /F /Q))"
						+ " & (if exist \"{1}\\binaryassetbuilder.sessioncache.xml.old\" (del \"{1}\\binaryassetbuilder.sessioncache.xml.old\" /F /Q))"
						+ " & (if exist \"{1}\\binaryassetbuilder.sessioncache.xml.deflate\" (del \"{1}\\binaryassetbuilder.sessioncache.xml.deflate\" /F /Q))"
						+ " & (if exist \"{1}\\binaryassetbuilder.sessioncache.xml.deflate.old\" (del \"{1}\\binaryassetbuilder.sessioncache.xml.deflate.old\" /F /Q))"
						+ " & (if exist \"{1}\\stringhashes.xml\" (del \"{1}\\stringhashes.xml\" /F /Q))",
						SDKDirectory, BuiltModsPath, BuiltModPath);
					BuildHelper.RunStep(StepType.RunExecutable, clearSessionCache);
                    break;
                case 3:
					BuildHelper.DisplayLine(String.Format("Step {0}: Clearing built mod",
							stepId));
                    RunExecutableArguments clearMod = new RunExecutableArguments(Cmd);
                    clearMod.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (for /R \"{1}\" %I in (\"*.*\") do ("
							+ "(if not \"%~xI\" == \".asset\" (del \"%I\" /F /Q))"
						+ "))",
						SDKDirectory, BuiltModPath);
					BuildHelper.RunStep(StepType.RunExecutable, clearMod);
                    break;
				case 4:
				    BuildHelper.DisplayLine("Step 4: Compiling AptUI XML with Rust");
				    RunExecutableArguments argsRustApt = new RunExecutableArguments(Cmd);
				    argsRustApt.Arguments = String.Format(
				        "/C (@echo off) & (if exist \"{1}\" (\"{0}\" compile-aptui \"{1}\" --data-root \"{2}\" --output \"{3}\" 2>&1))",
				        Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), Path.Combine(ModDataPath, "aptui"), SDKDirectory, Path.Combine(BuiltModDataPath, "aptui"));
				    argsRustApt.Arguments += " || (echo Critical: Build AptUI failed. & exit /b 1)";
				    RunRustBuildStep(argsRustApt);
				    break;
				case 5:
				    BuildHelper.DisplayLine("Step 5: Compiling global XML with Rust");
				    RunExecutableArguments argsRustGlobal = new RunExecutableArguments(Cmd);
				    argsRustGlobal.Arguments = String.Format(
				        "/C (@echo off) & (if exist \"{1}\" (\"{0}\" compile-global-sdk \"{1}\" --sdk-root \"{2}\" --output \"{3}\" --game-data \"{4}\" 2>&1))",
				        Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), Path.Combine(ModDataPath, "additionalmaps"), SDKDirectory, BuiltModsPath, RA3DataDir);
				    argsRustGlobal.Arguments += " || (echo Critical: Build Global Data failed. & exit /b 1)";
				    RunRustBuildStep(argsRustGlobal);
				    break;
				case 6:
				{
				    BuildHelper.DisplayLine("Step 6: Compiling static XML with Rust");
				    string rustStatic = Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe");
				    RunExecutableArguments argsRustStatic = new RunExecutableArguments(Cmd);
				    string pathError = StaticBuildPathError(rustStatic, ModXml);
				    if (pathError != null)
				    {
				        BuildHelper.DisplayLine("Critical: Build Static Data failed. " + pathError);
				        argsRustStatic.Arguments = "/D /C exit /b 1";
				        BuildHelper.RunStep(StepType.RunExecutable, argsRustStatic);
				        break;
				    }
				    string rustSdkRoot = Path.Combine(SDKDirectory, ".");
				    argsRustStatic.Arguments = String.Format(
				        "/C (@echo off) & (\"{0}\" compile-static-sdk \"{1}\" --sdk-root \"{2}\" --output \"{3}\" --game-data \"{4}\" 2>&1)",
				        rustStatic, ModXml, rustSdkRoot, BuiltModsPath, RA3DataDir);
				    if (!(bool)currentGUIData["step7"] && !(bool)currentGUIData["step8"])
				    {
				        argsRustStatic.Arguments += String.Format(" && (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1)", rustStatic, Path.Combine(BuiltModDataPath, "mod.manifest"), BuiltModsPath);
				    }
				    argsRustStatic.Arguments += " || (echo Critical: Build Static Data failed. & exit /b 1)";
				    RunRustBuildStep(argsRustStatic);
				    break;
				}
				case 7:
				{
				    BuildHelper.DisplayLine("Step 7: Merging precompiled assets with Rust");
				    RunExecutableArguments argsRustMerge = new RunExecutableArguments(Cmd);
				    string rustMerge = Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe");
				    string modManifest = Path.Combine(BuiltModDataPath, "mod.manifest");
				    const string mergeFormat = "(\"{0}\" merge-assets \"{1}\" --assets \"{2}\" 2>&1 || (echo Critical: Merge Assets failed. & exit /b 1))";
				    argsRustMerge.Arguments = String.Format(
				        "/C (@echo off) & (if exist \"{0}\" if exist \"{1}\" {2})",
				        modManifest, Path.Combine(ModAssetsPath, "mod"),
				        String.Format(mergeFormat, rustMerge, modManifest, Path.Combine(ModAssetsPath, "mod")));
				    foreach (string group in new string[] { "additionalmaps", "aptui" })
				    {
				        string pattern = group == "additionalmaps" ? "mapmetadata_*.manifest" : "*.manifest";
				        string assets = Path.Combine(Path.Combine(ModAssetsPath, group), "%~nI");
				        argsRustMerge.Arguments += String.Format(
				            " & (for %I in (\"{0}\") do (if exist \"%~fI\" if exist \"{1}\" {2}))",
				            Path.Combine(Path.Combine(BuiltModDataPath, group), pattern), assets,
				            String.Format(mergeFormat, rustMerge, "%~fI", assets));
				    }
				    if (!(bool)currentGUIData["step8"])
				    {
				        argsRustMerge.Arguments += String.Format(
				            " & (if exist \"{1}\" (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1 || (echo Critical: Fix Assets failed. & exit /b 1)))",
				            rustMerge, modManifest, BuiltModsPath);
				    }
				    RunRustBuildStep(argsRustMerge);
				    break;
				}
				case 8:
				    BuildHelper.DisplayLine("Step 8: Fixing linked assets with Rust");
				    RunExecutableArguments argsRustFix = new RunExecutableArguments(Cmd);
				    argsRustFix.Arguments = String.Format(
				        "/C (@echo off) & (\"{0}\" fix-assets \"{1}\" --references \"{2}\" --allow-missing-references 2>&1)"
				        + " || (echo Critical: Fix Assets failed. & exit /b 1)",
				        Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), Path.Combine(BuiltModDataPath, "mod.manifest"), BuiltModsPath);
				    RunRustBuildStep(argsRustFix);
				    break;
				case 9:
				{
				    BuildHelper.DisplayLine("Step 9: Compiling map overrides with Rust");
				    string maps = Path.Combine(ModAdditionalFilesPath, @"data\maps\official");
				    RunExecutableArguments argsMaps = new RunExecutableArguments(Cmd);
				    argsMaps.Arguments = String.Format(
				        "/C (@echo off) & (\"{0}\" compile-maps-sdk \"{1}\" --sdk-root \"{2}\" --references \"{3}\" --output \"{1}\" --game-data \"{4}\" --skip-precompiled 2>&1)"
				        + " || (echo Error: Could not run the map override build. & exit /b 0)",
				        Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), maps, SDKDirectory, BuiltModsPath, RA3DataDir);
				    RunRustBuildStep(argsMaps);
				    break;
				}
				case 10:
				    BuildHelper.DisplayLine(String.Format("Step {0}: Creating BIG files with Rust", stepId));
                    if (OverlapMiscBigs()) { FinishBigBuild(); break; }
				    RunExecutableArguments argsBig = new RunExecutableArguments(Cmd);
				    argsBig.Arguments = String.Format(
				        "/C (@echo off)"
				        + " && (if exist \"{1}\" (\"{0}\" pack-big \"{1}\" --output \"{2}\\{3}\" 2>&1))"
				        + " && (if exist \"{4}\" (\"{0}\" pack-big \"{4}\" --output \"{2}\\{5}\" 2>&1))"
				        + " || (echo ERROR Creating BIG files failed. & exit /b 1)",
				        Path.Combine(SDKDirectory, @"tools\binary-asset-builder.exe"), BuiltModPath, ModInstallPath, ModBigStreams, ModAdditionalFilesPath, ModBigMisc);
				    RunRustBuildStep(argsBig);
				    break;
                case 11:
					BuildHelper.DisplayLine(String.Format("Step {0}: Creating skudef file",
							stepId));
					RunExecutableArguments argsSkudef = new RunExecutableArguments(Cmd);
					argsSkudef.Arguments = String.Format("/C (@echo off) & (cd /D \"{0}\")"
						+ " & (if exist \"{0}\\{2}\" (del \"{0}\\{2}\" /F /Q))"
						+ " & (echo mod-game {3}>\"{2}\")"
						+ " & (if exist \"{0}\\{1}\" (echo add-big {1}>>\"{2}\"))"
						+ " & (if exist \"{0}\\{4}\" (echo add-big {4}>>\"{2}\"))",
						ModInstallPath, ModBigStreams, ModSkudef, DEFAULT_GAME_VERSION, ModBigMisc);
                    BuildHelper.RunStep(StepType.RunExecutable, argsSkudef);
                    break;
                default:
                    // unknown build step, this is probably the end
					onStepSuccess(-1);
                    break;
            }
        }
    }
}

//===================================================//
//===================================================//
//===================================================//
