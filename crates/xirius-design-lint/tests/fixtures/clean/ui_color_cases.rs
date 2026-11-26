// 透ける面と文字が別の枝にある。
fn f(){if flag {pen.round(r,fill.opacity(0.3),radius);} else {pen.text(s,size,weight,ink,r,align);}}
// ----
// 透ける面の上を、不透明な面で覆ってから文字を書く。
fn f(){pen.round(r,fill.opacity(0.3),radius);pen.round(r,surface,radius);pen.text(s,size,weight,ink,r,align);}
