from __future__ import annotations

from pathlib import Path
import json
import queue
import threading
import tkinter as tk
from tkinter import ttk, filedialog, messagebox

from .pipeline import run_full_pipeline
from .workspace import import_runtime_dump, build_azahar_pack
from .version import DISPLAY_VERSION

APP_TITLE=f'Etrian Odyssey HD Texture Extractor {DISPLAY_VERSION}'

class App(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title(APP_TITLE); self.geometry('1040x720'); self.minsize(900,620)
        self.configure(bg='#111318')
        self.q=queue.Queue(); self.worker=None
        self._style(); self._ui(); self.after(100,self._pump)

    def _style(self):
        s=ttk.Style(self)
        try: s.theme_use('clam')
        except tk.TclError: pass
        s.configure('.',background='#171a20',foreground='#e9edf2',fieldbackground='#20242c',bordercolor='#313743')
        s.configure('TFrame',background='#171a20'); s.configure('Card.TFrame',background='#1c2028')
        s.configure('TLabel',background='#171a20',foreground='#e9edf2')
        s.configure('Muted.TLabel',foreground='#9ba7b4')
        s.configure('Title.TLabel',font=('Segoe UI',20,'bold'),foreground='#f5f7fa')
        s.configure('Accent.TButton',font=('Segoe UI',10,'bold'),padding=10)
        s.configure('TButton',padding=8)
        s.configure('TEntry',padding=7)
        s.configure('TCheckbutton',background='#171a20')
        s.configure('TNotebook',background='#111318',borderwidth=0)
        s.configure('TNotebook.Tab',padding=(16,9))

    def _ui(self):
        root=ttk.Frame(self,padding=18); root.pack(fill='both',expand=True)
        ttk.Label(root,text='Etrian Odyssey HD Texture Extractor',style='Title.TLabel').pack(anchor='w')
        ttk.Label(root,text='0.13 legacy-reference hardening for EOU / EO2U → Azahar.',style='Muted.TLabel').pack(anchor='w',pady=(2,14))
        nb=ttk.Notebook(root); nb.pack(fill='both',expand=True)
        self.extract_tab=ttk.Frame(nb,padding=14); self.tools_tab=ttk.Frame(nb,padding=14); self.about_tab=ttk.Frame(nb,padding=14)
        nb.add(self.extract_tab,text='Extract'); nb.add(self.tools_tab,text='Workspace Tools'); nb.add(self.about_tab,text='Notes')
        self._extract_ui(); self._tools_ui(); self._about_ui()

    def _row(self,parent,label,var,kind='file'):
        f=ttk.Frame(parent); f.pack(fill='x',pady=6)
        ttk.Label(f,text=label,width=20).pack(side='left')
        ttk.Entry(f,textvariable=var).pack(side='left',fill='x',expand=True,padx=(0,8))
        def browse():
            if kind=='file': p=filedialog.askopenfilename()
            else: p=filedialog.askdirectory()
            if p: var.set(p)
        ttk.Button(f,text='Browse…',command=browse).pack(side='right')

    def _extract_ui(self):
        self.rom=tk.StringVar(); self.workspace=tk.StringVar(); self.forge=tk.StringVar(value=str(Path(__file__).resolve().parents[1]/'tools'/'3DS-Texture-Forge'))
        card=ttk.Frame(self.extract_tab,style='Card.TFrame',padding=14); card.pack(fill='x')
        self._row(card,'Decrypted EOU / EO2U ROM',self.rom,'file'); self._row(card,'Workspace folder',self.workspace,'dir'); self._row(card,'Texture Forge folder',self.forge,'dir')
        ttk.Label(self.extract_tab,text='Output: azahar_pack_master + azahar_pack (unique textures only)',style='Muted.TLabel').pack(anchor='w',pady=10)
        bar=ttk.Frame(self.extract_tab); bar.pack(fill='x',pady=(0,8))
        self.run_btn=ttk.Button(bar,text='Build Upscaling Workspace',style='Accent.TButton',command=self.run); self.run_btn.pack(side='left')
        self.progress=ttk.Progressbar(bar,mode='indeterminate'); self.progress.pack(side='left',fill='x',expand=True,padx=12)
        self.log=tk.Text(self.extract_tab,height=20,bg='#0d0f13',fg='#d8dee9',insertbackground='white',relief='flat',font=('Consolas',9),wrap='word')
        self.log.pack(fill='both',expand=True)

    def _tools_ui(self):
        self.tool_ws=tk.StringVar(); self.dump=tk.StringVar()
        self._row(self.tools_tab,'Workspace',self.tool_ws,'dir')
        self._row(self.tools_tab,'Azahar dump/old pack',self.dump,'dir')
        b=ttk.Frame(self.tools_tab); b.pack(fill='x',pady=10)
        ttk.Button(b,text='Rebuild deployment pack',command=self.rebuild).pack(side='left')
        ttk.Button(b,text='Import runtime hash evidence',command=self.import_hashes).pack(side='left',padx=8)
        self.tool_out=tk.Text(self.tools_tab,bg='#0d0f13',fg='#d8dee9',relief='flat',font=('Consolas',9),wrap='word'); self.tool_out.pack(fill='both',expand=True)

    def _about_ui(self):
        txt = f"""What this {DISPLAY_VERSION} legacy-final build does

• Keeps the verified EOU and EO2U extraction paths as the behavioral reference for the independent rewrite.
• Corrects BCH/PICA 8×8 storage sizing for non-aligned texture dimensions.
• Adds cross-platform archive path containment and shared depth/file/expanded-byte/member-size budgets.
• Makes azahar_pack_master recoverable even if the previous manifest is missing or corrupt.
• Preserves intentional pack.json renames and promotes master/deployment packs transactionally.
• Keeps exact runtime-hash matches verified while perceptual/upscaled matches remain candidates until confirmed.
• Labels reconstructed material alpha as diagnostic and removes dead transient paths from retained reports.
• Produces copyright-safe structural fingerprints for comparing future implementations.

Supported reference games

• Etrian Odyssey Untold: The Millennium Girl (EOU).
• Etrian Odyssey 2 Untold: The Fafnir Knight (EO2U).

The 0.13 Python application is being frozen as the reference implementation. Broader game support and removal of the Texture Forge/Python dependency belong to the independent Rust milestones that follow.

This program does not contain game assets, keys, ROMs, or Nintendo/Atlus code. Use a decrypted dump you created from your own copy."""
        t=tk.Text(self.about_tab,bg='#171a20',fg='#d8dee9',relief='flat',wrap='word',font=('Segoe UI',10))
        t.pack(fill='both',expand=True)
        t.insert('1.0',txt)
        t.configure(state='disabled')

    def _emit(self,s): self.q.put(('log',s))
    def run(self):
        if self.worker and self.worker.is_alive(): return
        rom=Path(self.rom.get()); ws=Path(self.workspace.get()); forge=Path(self.forge.get())
        if not rom.is_file(): messagebox.showerror(APP_TITLE,'Select a decrypted .3ds/.cia/.cxi ROM.'); return
        if not self.workspace.get(): messagebox.showerror(APP_TITLE,'Select a workspace folder.'); return
        self.run_btn.configure(state='disabled'); self.progress.start(12); self.log.delete('1.0','end')
        def job():
            try:
                res=run_full_pipeline(rom,ws,forge,self._emit,False,True)
                self.q.put(('done',res))
            except Exception as e: self.q.put(('error',str(e)))
        self.worker=threading.Thread(target=job,daemon=True); self.worker.start()

    def _pump(self):
        try:
            while True:
                typ,val=self.q.get_nowait()
                if typ=='log': self.log.insert('end',val+'\n'); self.log.see('end')
                elif typ=='done':
                    self.progress.stop(); self.run_btn.configure(state='normal')
                    self.log.insert('end','\n'+json.dumps(val,indent=2)+'\n'); self.tool_ws.set(self.workspace.get())
                    messagebox.showinfo(APP_TITLE,f"Workspace built for {val.get('game_short_name','Etrian Odyssey')}.\n{val['assets']} unique textures\nMaster: azahar_pack_master\nDeployment: azahar_pack\nTitle ID: {val['title_id']}")
                elif typ=='error':
                    self.progress.stop(); self.run_btn.configure(state='normal'); self.log.insert('end','\nERROR: '+val+'\n'); messagebox.showerror(APP_TITLE,val)
        except queue.Empty: pass
        self.after(100,self._pump)

    def import_hashes(self):
        try:
            r=import_runtime_dump(Path(self.tool_ws.get()),Path(self.dump.get())); self.tool_out.insert('end',json.dumps(r,indent=2)+'\n')
        except Exception as e: messagebox.showerror(APP_TITLE,str(e))
    def rebuild(self):
        try:
            p=build_azahar_pack(Path(self.tool_ws.get()),True); self.tool_out.insert('end',f'Deployment pack rebuilt: {p}\n')
        except Exception as e: messagebox.showerror(APP_TITLE,str(e))

def main(): App().mainloop()
