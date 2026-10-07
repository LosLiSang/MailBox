import { hue, initials } from "../format";

export function Avatar({ name, seed, size = 36 }: { name: string; seed: string; size?: number }) {
  return (
    <span
      className="avatar"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.42,
        background: `hsl(${hue(seed)} 55% 50%)`,
      }}
    >
      {initials(name)}
    </span>
  );
}
