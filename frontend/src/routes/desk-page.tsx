export function DeskPage() {
  return (
    <main className="dock-shell">
      <div className="dock-shell__field dock-shell__field--threads">
        <div className="threads-composer" role="group" aria-label="Eden prompt composer">
          <input
            className="threads-composer__input"
            type="text"
            placeholder="Message Eden"
          />
        </div>
      </div>
    </main>
  );
}
